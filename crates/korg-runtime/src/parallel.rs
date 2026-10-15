//! `run_parallel` — fan ONE task across N isolated git worktrees, run the honest
//! [`run_once`](crate::run_once) pipeline in each, pick a winner deterministically,
//! and seal the whole fan-out as ONE verifiable `korg-ledger@v1` journal.
//!
//! This is the "verifiable parallel runs" capability: where a normal agent IDE
//! lets you fan a prompt across N attempts and eyeball the diffs, korg records
//! the entire fan-out — every candidate's real, measured outcome and the winner
//! selection — as a hash-chained causal DAG that `korg-verify` accepts. The run
//! is not merely compared; it is *provable*.
//!
//! Each candidate's own run writes its per-worktree `korg-ledger@v1` journal
//! (see [`run_once`]); this module adds the PARENT journal that links them:
//! `user_prompt → {candidate}×N → winner_selected`. The events reuse the exact
//! conformance-tested chain primitives ([`chain_hash`]/[`GENESIS_HASH`]) the
//! single-run path uses, so the fan-out journal verifies byte-identically.

use crate::run_once::{run_once_honest_with, HonestRunReport};
use korg_llm::LlmProvider;
use korg_registry::ledger_chain::{chain_hash, GENESIS_HASH};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One candidate's measured outcome — derived from its [`HonestRunReport`], never
/// fabricated. The fields the deterministic winner selection reads are all real
/// observations: did it compile, how much did it actually change.
#[derive(Debug, Clone)]
pub struct CandidateReport {
    /// 0-based index in the fan-out.
    pub index: usize,
    /// The branch the candidate's worktree was created on (mergeable for the winner).
    pub branch: String,
    /// The candidate's isolated worktree path.
    pub worktree_path: String,
    /// Real number of files the candidate changed on disk.
    pub files_changed: usize,
    /// `"Passed"`, `"Failed"`, or `"Unavailable"` — the honest compile observation.
    pub cargo_check: String,
    /// Path to the candidate's own per-run korg-ledger@v1 journal, if written.
    pub ledger_path: Option<String>,
}

impl CandidateReport {
    fn from_report(index: usize, branch: String, worktree: &Path, r: &HonestRunReport) -> Self {
        CandidateReport {
            index,
            branch,
            worktree_path: worktree.display().to_string(),
            files_changed: r.files_changed,
            cargo_check: r.cargo_check.clone(),
            ledger_path: r.ledger_path.as_ref().map(|p| p.display().to_string()),
        }
    }

    /// Did the candidate's result compile? (the honest `cargo_check` observation)
    pub fn compiles(&self) -> bool {
        self.cargo_check == "Passed"
    }

    /// A real, non-empty change actually landed on disk.
    pub fn made_changes(&self) -> bool {
        self.files_changed > 0
    }
}

/// The outcome of a fan-out: the per-candidate reports, the chosen winner (if any
/// candidate produced a usable change), and the path to the sealed fan-out journal.
#[derive(Debug, Clone)]
pub struct ParallelOutcome {
    pub candidates: Vec<CandidateReport>,
    /// Index into `candidates` of the winner, or `None` if every candidate was an
    /// honest null (no change) — we never invent a winner that didn't do work.
    pub winner_index: Option<usize>,
    /// Human-readable reason the winner was chosen (for the journal + the CLI).
    pub winner_reason: String,
    /// Path to the verifiable korg-ledger@v1 fan-out journal.
    pub journal_path: Option<PathBuf>,
}

/// Pick the winning candidate deterministically — no extra model call, no
/// randomness, so the same set of outcomes always yields the same winner.
///
/// Order of preference:
///   1. Candidates that **compile** AND **made a real change**.
///   2. If none compile-with-changes, any candidate that made a change.
///   3. Within the pool, the **smallest real diff** (Occam: the minimal change
///      that achieves the task), ties broken by lowest index for determinism.
///
/// Returns `None` only when every candidate is an honest null (no change) — there
/// is genuinely nothing to pick. A future criterion (an LLM judge over the diffs)
/// can replace this without touching the fan-out/seal machinery.
pub fn select_winner(candidates: &[CandidateReport]) -> Option<usize> {
    if candidates.is_empty() {
        return None;
    }

    let compiled: Vec<&CandidateReport> = candidates
        .iter()
        .filter(|c| c.compiles() && c.made_changes())
        .collect();

    let pool: Vec<&CandidateReport> = if !compiled.is_empty() {
        compiled
    } else {
        let changed: Vec<&CandidateReport> =
            candidates.iter().filter(|c| c.made_changes()).collect();
        if changed.is_empty() {
            return None;
        }
        changed
    };

    pool.iter()
        .min_by(|a, b| {
            a.files_changed
                .cmp(&b.files_changed)
                .then(a.index.cmp(&b.index))
        })
        .map(|c| c.index)
}

/// Why the winner was chosen, in one line for the journal + CLI.
fn winner_reason(candidates: &[CandidateReport], winner: Option<usize>) -> String {
    match winner {
        None => "no winner: every candidate was an honest null (no change)".to_string(),
        Some(i) => {
            let w = &candidates[i];
            if w.compiles() {
                format!(
                    "candidate {i} — compiles (cargo check Passed) with the smallest real diff ({} file(s))",
                    w.files_changed
                )
            } else {
                format!(
                    "candidate {i} — smallest real diff ({} file(s)); no candidate compiled",
                    w.files_changed
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Verifiable fan-out journal (korg-ledger@v1)
// ---------------------------------------------------------------------------

/// One korg-ledger@v1 event in the flat on-disk shape the verifier accepts — the
/// exact shape [`run_once`](crate::run_once) emits, so the fan-out journal and the
/// per-run journals are the same dialect.
fn event(
    seq: u64,
    tool: &str,
    args: Value,
    result: Value,
    triggered_by: Option<u64>,
) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    m.insert("schema_version".into(), json!("1.0"));
    m.insert("seq_id".into(), json!(seq));
    m.insert("source_agent".into(), json!("agent:korg-parallel"));
    m.insert("tool_name".into(), json!(tool));
    m.insert("args".into(), args);
    m.insert("result".into(), result);
    m.insert("success".into(), json!(true));
    m.insert("duration_ms".into(), json!(0));
    if let Some(tb) = triggered_by {
        m.insert("triggered_by".into(), json!(tb));
    }
    m
}

/// Append one hash-chained event, computing its `entry_hash` from the previous
/// tip via the conformance-tested [`chain_hash`] primitive.
fn push_event(events: &mut Vec<Value>, prev: &mut String, mut e: serde_json::Map<String, Value>) {
    e.insert("prev_hash".into(), json!(prev.clone()));
    let value = Value::Object(e);
    let hash = chain_hash(&value, None);
    let mut obj = value.as_object().cloned().unwrap_or_default();
    obj.insert("entry_hash".into(), json!(hash));
    *prev = hash;
    events.push(Value::Object(obj));
}

/// Build the fan-out journal events: the operator prompt as the root, one event
/// per candidate (each `triggered_by` the prompt), and a `winner_selected` event
/// (`triggered_by` the winning candidate's event). The result is a well-formed
/// causal DAG — every `triggered_by` references a strictly-earlier `seq_id` — so
/// both `verify_chain` and `verify_dag` pass.
pub fn build_fanout_events(
    task: &str,
    candidates: &[CandidateReport],
    winner: Option<usize>,
    reason: &str,
) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    let mut prev = GENESIS_HASH.to_string();

    // seq 1: the operator's prompt, fanned across N candidates.
    push_event(
        &mut events,
        &mut prev,
        event(
            1,
            "parallel_prompt",
            json!({ "prompt": task, "candidates": candidates.len() }),
            json!({}),
            None,
        ),
    );

    // seq 2..N+1: one event per candidate's measured outcome. seq_id = index + 2.
    for c in candidates {
        let seq = c.index as u64 + 2;
        push_event(
            &mut events,
            &mut prev,
            event(
                seq,
                "parallel_candidate",
                json!({ "index": c.index, "branch": c.branch, "worktree": c.worktree_path }),
                json!({
                    "files_changed": c.files_changed,
                    "cargo_check": c.cargo_check,
                    "ledger": c.ledger_path,
                }),
                Some(1),
            ),
        );
    }

    // final: the winner selection, caused by the winning candidate's event.
    let final_seq = candidates.len() as u64 + 2;
    let triggered = winner.map(|i| i as u64 + 2).or(Some(1));
    push_event(
        &mut events,
        &mut prev,
        event(
            final_seq,
            "winner_selected",
            json!({ "strategy": "compiles-then-smallest-diff" }),
            json!({ "winner_index": winner, "reason": reason }),
            triggered,
        ),
    );

    events
}

/// Persist the fan-out journal to `<repo>/.korg/parallel-run.jsonl`.
fn write_fanout_journal(repo_path: &Path, events: &[Value]) -> std::io::Result<PathBuf> {
    let dir = repo_path.join(".korg");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("parallel-run.jsonl");
    let mut body = String::new();
    for e in events {
        body.push_str(&serde_json::to_string(e).unwrap_or_default());
        body.push('\n');
    }
    std::fs::write(&path, body)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Worktree fan-out orchestration
// ---------------------------------------------------------------------------

/// Create an isolated git worktree of `repo` on a fresh branch and return its path.
/// Best-effort: returns `None` if `git worktree add` fails (e.g. not a git repo).
async fn worktree_add(repo: &Path, branch: &str, dest: &Path) -> Option<PathBuf> {
    let status = tokio::process::Command::new("git")
        .current_dir(repo)
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &dest.display().to_string(),
            "HEAD",
        ])
        .output()
        .await
        .ok()?;
    if status.status.success() {
        Some(dest.to_path_buf())
    } else {
        None
    }
}

/// Tear down a worktree + its branch (best-effort; never fails the run).
async fn worktree_remove(repo: &Path, branch: &str, path: &Path) {
    let _ = tokio::process::Command::new("git")
        .current_dir(repo)
        .args(["worktree", "remove", "--force", &path.display().to_string()])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(repo)
        .args(["branch", "-D", branch])
        .output()
        .await;
}

/// Fan `task` across `n` isolated worktrees of `repo_path`, run the honest
/// pipeline in each with `provider`, pick a winner, and seal the fan-out into one
/// verifiable journal. Loser worktrees are cleaned up; the winner's worktree +
/// branch are kept so it can be reviewed and merged.
pub async fn run_parallel(
    task: &str,
    repo_path: &Path,
    n: usize,
    provider: &dyn LlmProvider,
) -> ParallelOutcome {
    let n = n.max(1);
    let base = std::env::temp_dir().join(format!("korg-parallel-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);

    let mut candidates: Vec<CandidateReport> = Vec::with_capacity(n);
    let mut worktrees: Vec<(String, PathBuf)> = Vec::with_capacity(n);

    for i in 0..n {
        let branch = format!("korg/parallel/{}/cand-{}", std::process::id(), i);
        let dest = base.join(format!("cand-{i}"));
        let Some(wt) = worktree_add(repo_path, &branch, &dest).await else {
            // Could not isolate this candidate — record an honest null and move on.
            candidates.push(CandidateReport {
                index: i,
                branch: branch.clone(),
                worktree_path: dest.display().to_string(),
                files_changed: 0,
                cargo_check: "Unavailable".to_string(),
                ledger_path: None,
            });
            continue;
        };
        let report = run_once_honest_with(task, &wt, provider).await;
        candidates.push(CandidateReport::from_report(i, branch.clone(), &wt, &report));
        worktrees.push((branch, wt));
    }

    let winner_index = select_winner(&candidates);
    let reason = winner_reason(&candidates, winner_index);

    // Seal the whole fan-out into one verifiable journal in the source repo.
    let events = build_fanout_events(task, &candidates, winner_index, &reason);
    let journal_path = write_fanout_journal(repo_path, &events).ok();

    // Clean up losers; keep the winner's worktree + branch for review/merge.
    let keep = winner_index.map(|i| candidates[i].worktree_path.clone());
    for (branch, wt) in worktrees {
        if Some(wt.display().to_string()) == keep {
            continue;
        }
        worktree_remove(repo_path, &branch, &wt).await;
    }

    ParallelOutcome {
        candidates,
        winner_index,
        winner_reason: reason,
        journal_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(index: usize, files: usize, check: &str) -> CandidateReport {
        CandidateReport {
            index,
            branch: format!("b{index}"),
            worktree_path: format!("/tmp/cand-{index}"),
            files_changed: files,
            cargo_check: check.to_string(),
            ledger_path: None,
        }
    }

    #[test]
    fn winner_prefers_compiling_candidate_with_changes() {
        // c0: bigger compiling diff, c1: smaller compiling diff, c2: failed.
        let c = vec![cand(0, 3, "Passed"), cand(1, 1, "Passed"), cand(2, 1, "Failed")];
        assert_eq!(select_winner(&c), Some(1), "smallest compiling diff wins");
    }

    #[test]
    fn winner_falls_back_to_changed_when_none_compile() {
        let c = vec![cand(0, 0, "Failed"), cand(1, 2, "Failed"), cand(2, 5, "Unavailable")];
        assert_eq!(select_winner(&c), Some(1), "smallest real change wins when none compile");
    }

    #[test]
    fn no_winner_when_all_honest_null() {
        let c = vec![cand(0, 0, "Passed"), cand(1, 0, "Unavailable")];
        assert_eq!(select_winner(&c), None, "a run that changed nothing is not a winner");
    }

    #[test]
    fn ties_break_to_lowest_index_for_determinism() {
        let c = vec![cand(0, 2, "Passed"), cand(1, 2, "Passed")];
        assert_eq!(select_winner(&c), Some(0));
    }

    #[test]
    fn empty_pool_has_no_winner() {
        assert_eq!(select_winner(&[]), None);
    }

    #[test]
    fn fanout_journal_is_hash_chained_and_well_formed() {
        let c = vec![cand(0, 1, "Passed"), cand(1, 2, "Passed")];
        let winner = select_winner(&c);
        let reason = winner_reason(&c, winner);
        let events = build_fanout_events("do the thing", &c, winner, &reason);

        // prompt + N candidates + winner_selected
        assert_eq!(events.len(), 1 + c.len() + 1);

        // Each event chains to the previous tip: re-derive entry_hash and compare,
        // exactly as verify_chain does.
        let mut prev = GENESIS_HASH.to_string();
        for e in &events {
            let obj = e.as_object().unwrap();
            assert_eq!(obj.get("prev_hash").unwrap().as_str().unwrap(), prev);
            let mut bare = obj.clone();
            bare.remove("entry_hash");
            let expected = chain_hash(&Value::Object(bare), None);
            let got = obj.get("entry_hash").unwrap().as_str().unwrap();
            assert_eq!(got, expected, "entry_hash must chain from prev tip");
            prev = got.to_string();
        }

        // The winner event records the selected index and is caused by it.
        let last = events.last().unwrap();
        assert_eq!(last["tool_name"], json!("winner_selected"));
        assert_eq!(last["result"]["winner_index"], json!(winner));
        // winner index 0 → its candidate event is seq_id 2 → winner triggered_by 2.
        assert_eq!(last["triggered_by"], json!(2));
    }
}
