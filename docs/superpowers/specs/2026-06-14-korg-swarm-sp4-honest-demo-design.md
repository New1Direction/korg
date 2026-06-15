# Korg Swarm — SP4: Honest Demo + visible honest pipeline (Track B)

**Status:** Design+plan / approved-by-delegation ("do it all now"), grounded against real code 2026-06-14
**Branch:** `feat/swarm-honest-demo` (stacked on `feat/swarm-honest-pipeline`)
**Sub-project:** SP4 of Track B.

> **Pivotal grounding finding:** the orchestrated `korg campaign` does NOT do real work — its 4 worker children idle 20s, get killed (`exit -1`), get a *faked* recovery, and attest `total_mutations_so_far: 0`. Benjamin's DAG package is literally `"Implement (simulate-crash): …"`. The SP1 honest pipeline is real but only fires *below* this broken orchestration. So an honest demo **cannot record the campaign** — it must expose the honest pipeline through a **real, working entrypoint**. Fixing the campaign itself is SP2 territory.

## 1. Goal
Make SP1's honest pipeline **user-visible and runnable**, and replace the faked hero demo with one that records the **real binary** doing real, verifiable work.

## 2. Deliverables (3 honest beats, zero simulation)

### D1 — `korg run-once` (the real entrypoint) — the headline
A new `Commands::RunOnce { task, repo }` subcommand that drives the SP1 honest pipeline visibly:
1. Resolve a target repo (default: a temp copy of `fixtures/honest-demo-repo`, `git init`+commit — the exact dance `crates/korg-runtime/tests/honest_pipeline.rs:50-73` already does; reuse it).
2. Run the honest pipeline for Benjamin on `task` using the default `DeterministicProvider`: build the Benjamin system prompt (so `role_marker` → "benjamin"), `provider.complete` → parse → `observation::apply_mutations` → `numstat` → `cargo_check` → `honest_metrics`.
3. Print an honest attestation block: `files_changed`, `cargo check` result (PASSED/FAILED/UNAVAILABLE), and **"attested mutation count = N (== real git diff)"**.
4. Write a verifiable **korg-ledger@v1** ledger of the events (reuse the existing producer path so `korg-verify` / the in-browser verifier accept it), printing its path.

**Honesty guardrails:** if the task isn't the fixture task, `DeterministicProvider` returns an honest-null → run-once prints `files_changed=0, attested 0` truthfully (never fabricates). The output must equal what the keystone test asserts.

### D2 — README honesty fixes
- `README.md:12` hero alt text: drop the false **"fork"** claim → e.g. "record, verify, and rewind an AI agent session as a hash-chained ledger".
- Soften/remove **"fork"** in README:48 and the phantom `korg fork` / `korg checkpoints list|restore` commands (README:181-192) that **do not exist** in `enum Commands`. Either delete those lines or mark them "planned". (No `korg fork` is built in SP4 — see §4.)

### D3 — Honest demo recording
- Rewrite `demo.tape`: type AND run the **real** binary — `korg run-once` (real attestation) then real `korg rewind --seq N` on a real journal, then `git log`/verify showing the snap-back. Remove every `Type "/vhs/demo-sim.sh …"` line.
- Delete `demo-sim.sh` (the fabrication source) so the tape can't be re-pointed at it.
- Regenerate `demo.gif`/`demo.mp4`/`demo.webp` with VHS+ffmpeg (installed locally). Retune `Sleep` to real command durations. *(If GIF regeneration is environment-blocked in this run, ship D1+D2 + the rewritten `.tape` + deleted sim, and note the GIF as a manual re-record step — never ship a regenerated GIF that still embeds sim output.)*

## 3. Plan (TDD, bite-sized)

**Build order:** D1 (run-once, the substance) → D2 (README) → D3 (tape/sim/GIF).

### Task 1 — `korg run-once` subcommand (TDD via an integration test)
- **Files:** `src/main.rs` (add `Commands::RunOnce { task, repo }` + handler); a new `korg-runtime` helper `pub async fn run_once_honest(task, repo_path) -> HonestRunReport` in a new `crates/korg-runtime/src/run_once.rs` (so the logic is testable without the CLI); `crates/korg-runtime/tests/run_once.rs` (integration test).
- **RED:** test asserts `run_once_honest("Fix the add function in src/lib.rs so it adds", <temp fixture copy>)` returns a report with `files_changed == 1`, `cargo_check == Passed`, `attested_count == 1`, `attested_count == numstat_files`. (Mirror `honest_pipeline.rs` setup.)
- **GREEN:** implement `run_once.rs` reusing `korg_llm::DeterministicProvider` + `korg_runtime::observation::*` + `korg_runtime::personas::parse_structured_response`; build the Benjamin system prompt via `load_prompt_for_persona`/the persona's marker so role resolves to "benjamin". Write the korg-ledger@v1 ledger via the existing writer.
- Wire `Commands::RunOnce` in `main.rs` to call it and pretty-print the report.
- Commit `feat(cli): korg run-once — drive the honest pipeline visibly on a fixture`.

### Task 2 — README honesty
- Edit `README.md:12`, `:48`, `:181-192` per D2. No code. Commit `docs: drop phantom fork/checkpoints claims; honest hero alt text`.

### Task 3 — honest tape + delete sim
- Rewrite `demo.tape` (real commands only), `git rm demo-sim.sh`. Commit `chore(demo): honest tape runs the real binary; delete demo-sim.sh`.
- Attempt GIF regen (`vhs demo.tape`); if it produces clean real output, commit the new assets. If blocked, commit the tape+sim-deletion and leave a `demo/RERECORD.md` note.

## 4. Scope / decisions (autonomous, per "do it all")
- **Cut the fork beat.** A real `korg fork` is a separate M-L effort (checkpoint primitives exist `pub(crate)` but nothing wires `branch_id`); building it is out of SP4. README softened to "rewind" only.
- **run-once, not campaign.** The campaign attests 0 (broken workers); fixing it is **SP2**. run-once is the smallest honest spine that shows real attested work.
- **Reuse the keystone setup** (temp fixture + git init) verbatim — it's proven.
- Leave the scripted `korg demo` (math_utils.py time-travel, main.rs:1783) as-is for now; note it as a separate fabrication to address.

## 5. Verification
- `cargo test -p korg-runtime --test run_once` passes (attested == real diff == 1).
- `korg run-once "Fix the add function in src/lib.rs so it adds"` prints `files_changed=1 · cargo check=PASSED · attested mutation count=1` and a ledger path; `korg-verify <ledger>` accepts it.
- `korg run-once "something unrelated"` prints `files_changed=0 · attested 0` (honest-null), no fabrication.
- `grep -rn "fork" README.md` shows no remaining false capability claim; `demo-sim.sh` gone; `demo.tape` contains no `demo-sim.sh` reference.
- Full `cargo test --workspace` green; fmt+clippy clean.

## 6. Out of scope (later)
- Fix the campaign worker path so the orchestrated swarm does real work (**SP2** — its hidden prerequisite).
- Real `korg fork` / `korg checkpoints` CLI.
- Making `korg demo` (math_utils) real.
