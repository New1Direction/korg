# Korg Swarm — SP1: The Honest Pipeline (Track B, keystone)

**Status:** Design / approved for planning (integration claims verified against code 2026-06-14)
**Date:** 2026-06-14
**Branch (proposed):** `feat/swarm-honest-pipeline` (stacked off the current work)
**Sub-project:** SP1 of Track B ("make the swarm real"). SP2 (collaboration data-flow), SP3 (real warm boot), SP4 (honest demo) are out of scope here and get their own spec → plan → build cycles.

> **Verification note.** Every `file:line` and "reuse vs net-new" claim below was adversarially checked against the actual `korg-runtime` / `korg-llm` source (three independent reviewers). Where the first draft assumed reuse of code that doesn't exist or is wired differently, this version says so explicitly and marks the work **NET-NEW**.

---

## 1. Context & motivation

Korg has two halves: a genuinely world-class **verifiable ledger / flight-recorder** (just hardened across five adversarial passes — 34 bugs fixed, three independent verifiers proven byte-identical) and a **swarm / "cognitive hypervisor"** that the 2026-06-12 audit flagged as theatrical.

A fresh code map (2026-06-14) refined the diagnosis. The important finding:

> **The cryptographic spine is real, and it signs fabricated numbers.**

What is already real:
- Workers are real OS subprocesses (`korg worker` children spawned via `current_exe()`, `session.rs:315`) in isolated **git worktrees** with zero-trust `git write-tree` merkle checks (`harness.rs:240,267`).
- The `.ktrans` ledger is genuine: JCS sha256 content-addressing, Ed25519 signing, hash-chaining (`leader.rs:612` `persist_campaign_ktrans`).
- `korg-ledger::verify_chain` / `verify_event_sig` truly detect tampering (proptest-backed).
- The Evaluator's `semantic_entropy` is real — pairwise cosine over an embedding window, five threshold rubrics (`evaluator.rs:207`).
- Personas have distinct prompts, output schemas, temperatures, provider overrides, and DAG roles (the audit's "differ only by temperature" claim is now outdated/false).

What is theater — all of it in the **data** flowing through the real spine:
1. The default `MockProvider` returns the literal string `[Mock Response to: "…"]`, ignoring prompt + temperature (`korg-llm/lib.rs:201`). It returns `Ok(...)`, so the persona's `think()` *succeeds*; `parse_structured_response` finds no JSON and leaves **empty mutations + a generic default confidence (0.85)** (`personas.rs:168,282`). *(Correction vs. the audit: the hand-coded `fallback_*` constants at `personas.rs:361` fire only on a provider `Err` — not on the mock's `Ok`. The default hermetic path produces **zero parsed mutations**, not the canned 7-file constants. The "no real signal" conclusion holds; the mechanism is "empty parse," not "fallback constants.")*
2. The worker emits **no real metrics** — `harness.rs:319` packs only `per_agent: {worker_id: {"phase":"start"/"complete"}}` into the pulse. The Evaluator never sees real swarm signal.
3. Because of (2), the loop **injects synthetic signal**: a hardcoded `stress_event` (risk 0.71, tuned just over the fail thresholds, `leader.rs:1519`), a per-round `sin()` TraceEvent (`leader.rs:1718`), a background `captain-async-planner` ingest (`leader.rs:908`), and **sin/cos jitter** added on top of the *real* arena scores in the TUI telemetry (`leader.rs:1607`). There is also dead synthetic code — `build_live_evolving_pulse` (`harness.rs:581`) is never called.
4. Ledger fields are decorative: `total_mutations_so_far: (round+1)*5` (`leader.rs:680`); `severity`/`remediation_confidence` are closed-form functions of `arena_confidence`; `blast_radius` is a closed-form function of `mutations_this_round` (`leader.rs:687`) — which is itself synthetic (`arena_outcome["mutations"].as_u64().unwrap_or(3) + round%2`, `leader.rs:1743`).

**Thesis.** Making the swarm "real" is not about superhuman cognition. It is about making the **verifiable ledger attest to facts the loop actually observed** — the exact thesis the trust core already embodies, extended one layer up. A flight-recorder that records lies is worthless.

## 2. Goal

Make one persona's work flow **honestly** end-to-end, so that a default, hermetic `korg campaign` produces a `.ktrans` ledger whose attested numbers are **real measurements** a verifier can trust:

> Benjamin (the implementer) does real observable work in its worker subprocess → the worker applies it and measures reality → emits real telemetry → the Evaluator scores only real signal → the ledger attests facts derived from the real diff.

**The acid test:** the attested `mutations_this_round` **equals the actual file count of the real `git diff --numstat`**, and the default campaign path contains **zero fabricated telemetry**.

### Non-goals (this spec)
- Generalizing to all five personas (→ SP2).
- Inter-persona data-flow / collaboration (→ SP2).
- Real warm boot / SandboxPool (→ SP3).
- Recording the headline demo (→ SP4).
- Real cognition quality. The deterministic stub is **honest, not smart**; a real LLM is opt-in.
- Byte-identical *ledger* output (the ledger legitimately stamps real clocks/UUIDs). Only `DeterministicProvider.complete()`'s text output is byte-stable.

## 3. Key decisions (resolved during brainstorming)

- **Definition of "real" = honest observation, provider-agnostic.** The default is a deterministic, hermetic stub; a real local model (`--provider ollama`) is opt-in. The observation → telemetry → ledger layer is identical for both.
- **Scope = thin honest spine.** All three units, proven through Benjamin only, **in the worker child process**. Low blast radius; SP2 generalizes.
- **Synthetics are gated, not deleted.** The fully-synthetic injectors move behind `--inject-stress` + a seeded scenario file. The one *real-base* site (`leader.rs:1607`) keeps its real scores and just loses its jitter.
- **`MockProvider` stays; a new default provider is added.** `MockProvider` is a programmable response-queue unit tests rely on — untouched. The new `DeterministicProvider` becomes the hermetic *campaign* default.
- **"Honest null" principle.** The deterministic stub must never fabricate success. On a task it has no canonical answer for, it returns an honest null → 0-mutation, low-confidence telemetry. **This also requires de-fabricating the downstream defaults** (§4 Unit C) — today the blackboard invents values for missing fields, which would silently undo an honest null.

## 4. Architecture — three isolated units

```
                ┌──────────────── Unit A: cognition (swappable) ─────────────────┐
   task ─────►  │  DeterministicProvider (default, hermetic)  |  Ollama (opt-in) │
                └───────────────────────────────┬────────────────────────────────┘
                                                │ structured output w/ APPLYABLE patch (or honest-null)
                                                ▼   [runs inside the korg worker child]
                ┌──────────── Unit B: observation (provider-agnostic) ───────────┐
                │  APPLY patch to worktree (NET-NEW) → git diff --numstat →       │
                │  3-state cargo check · real worker-local tokens · apply-        │
                │  cleanliness · sysinfo load · real surface_text                 │
                │  → pack real fields into per_agent JSON + plumb numstat up      │
                └───────────────────────────────┬────────────────────────────────┘
                                                │ AcpMessage::SwarmTelemetryPulse + PersonaResult.numstat
                                                ▼   [back in the leader process]
                ┌──────────── Unit C: honest scoring + ledger ───────────────────┐
                │  metrics_to_trace_event (DE-FABRICATE defaults) → Evaluator     │
                │  scores REAL signal; fully-synthetic injectors gated behind     │
                │  --inject-stress; :1607 keeps real scores, drops jitter;        │
                │  mutations_this_round (call site :1743) = plumbed-up real numstat│
                └────────────────────────────────────────────────────────────────┘
```

### Unit A — Honest deterministic provider (`korg-llm`)

A new `LlmProvider` impl (`DeterministicProvider`) satisfying the existing trait (`lib.rs:158`: `name`/`complete`/`complete_stream`).

- **Role recovery.** `LlmRequest` (`lib.rs:69`) has **no** role field — the persona system prompt is free text in `req.messages[0].content` (built from `load_prompt_for_persona`, `personas.rs:249`). `DeterministicProvider` recovers the role by matching a **stable marker** in that system text (e.g. Benjamin's "Builder & Implementer", `personas.rs:182`). *(Adding a structured role to `LlmRequest` would ripple through every provider + call site — out of scope.)*
- **Output.** Keyed on `(role, task fingerprint, seed)`. For the committed **fixture task**, Benjamin's output is a structured `mutations` array carrying an **explicit applyable field** (`content` = full new-file bytes, or a real unified `patch`) — *not* the free-text `description` the current schema uses. `Prompts/benjamin.md`'s schema is extended to include this field.
- **Honest null.** For an unknown task, return a well-formed `mutations: []` with an explicit `note` — parsed as zero mutations, never fabricated success.
- **Truthful usage.** `TokenUsage` reflects the bytes actually emitted, not the `25` constant.
- **Hermetic.** `complete()` is a pure function of its inputs → byte-identical output (CI-gateable). Temperature seeds the (deterministic) selection so "temperature changes the distribution" holds without intra-run nondeterminism.
- **Wiring (must land together).** Add a `"deterministic" => Arc::new(DeterministicProvider::new(...))` arm in `build_provider_with` (`lib.rs:2104` match; the wildcard `_ => MockProvider` at `lib.rs:2204` means a default flip alone is inert), and flip the campaign default at **both** `KorgConfig::default` (`lib.rs:1853`) and `KorgConfig::load` (`lib.rs:2051`) from `"mock"` to `"deterministic"`. `MockProvider` remains constructible and unchanged.

### Unit B — Observation layer (in the worker child, `harness.rs::run_task_in_worktree`)

The insertion point is `run_task_in_worktree` (`harness.rs:521`) — the **worker-child body** that owns the worktree CWD (`harness.rs:267`) and the parsed persona output (`persona_result.mutations`, from the `run_persona` call at `harness.rs:530`, `run_persona` itself defined in `personas.rs:465`). It must run **in the child**, not in `run_persona` (which also executes in the parent leader for Evaluator/arena calls, `leader.rs:2763/2975/3251`). Today this function calls `run_persona` then immediately `git add .` + `git write-tree` **without applying the mutations** (`harness.rs:533`), so the snapshot is the unmodified base. SP1 inserts apply + measure between the `run_persona` call (`:530`) and the `Ok(TaskResult{...})` construction (`:552`):

1. **Apply (NET-NEW).** Parse Benjamin's `mutations` and write / `git apply` the applyable patch field to the worktree files. Record whether it applied cleanly or produced rejects. *(No existing API does this — `workspace.rs`'s `WorkspaceManager` only does create/snapshot/restore and is not even used by the live worker. This is new code in the worker body.)*
2. **Measure** (all real, all local, all in-child):
   - `git diff --numstat` (NET-NEW helper) → files touched, lines ±.
   - **3-state cargo check** (NET-NEW; the existing `get_cargo_check_stderr` at `workers.rs:831` is module-private and returns `Option<String>` = `Some` only on failure, `None` on *both* success and cargo-absent — it cannot tell "passed" from "missing"). New helper returns `Passed | Failed(stderr) | Unavailable`, `pub(crate)` so the harness can call it.
   - **Worker-local token usage** — thread the real `usage` out of `provider.complete()` (today `run_persona` discards the `LlmResponse`, keeping only `.content`, `personas.rs:280`; add a tokens field to `PersonaResult`) and divide by worker-local elapsed. *(`korg_llm::CAMPAIGN_TOKENS` is a per-process atomic in the leader; the worker is a different process — so this must be measured child-side.)*
   - `sysinfo` CPU-load probe (NET-NEW crate dependency) → `gpu_util`, honestly a compute-load proxy. Fallback `0.0` only if the probe API errors (the crate is compiled in, so it's never "absent" at runtime).
   - Real `surface_text` = a summary of the actual diff + Benjamin's real output (→ fed to the genuine `semantic_entropy` embedding).
3. **Emit + plumb.** Pack the real fields as JSON keys into the `per_agent[worker_id]` object of `AcpMessage::SwarmTelemetryPulse` (`acp.rs:351` — the real wire message; `SwarmTelemetryPulsePayload` at `acp.rs:418` is dead code and is *not* used), replacing `{"phase":"start"}`. Additionally surface the real numstat file count up to the leader: include it in the worker's `SubmitTransaction`/result so `spawn_worker_process` (`workers.rs:766`) can attach it to `PersonaResult` (Unit C consumes it). Delete the dead `build_live_evolving_pulse`.

The honest mapping (the heart of SP1), packed as `per_agent` JSON keys:

| pulse key | Honest source |
|---|---|
| `risk_score` | cargo check **Failed → ~0.75**, **Passed → ~0.2**, scaled by normalized blast radius |
| `epistemic_confidence` | Passed (+ tests green) → high; Failed / empty / honest-null → low |
| `conflict_rate` | patch applied cleanly → `0.0`; `git apply` rejects → proportion rejected |
| `token_velocity` | **real** worker-local tokens ÷ worker-local wall-clock |
| `gpu_util` | real `sysinfo` load proxy; `0.0` only on probe error |
| `verified_count_delta` | real count of checks/tests passed (cargo Passed = +1, …) |
| `authority_improvement` | derived from `verified_count_delta` sign; `0.0` when nothing improved |
| `surface_text` | real diff summary + real persona output |

All mappings are named constants/helpers (no inline magic numbers), tunable + testable.

### Unit C — Honest scoring + ledger (`leader.rs` / `blackboard.rs` / `evaluator.rs`)

1. **De-fabricate the trace mapping (REQUIRED).** `metrics_to_trace_event` (`blackboard.rs:282`) currently `unwrap_or`s invented defaults for missing fields (risk 0.35, gpu 0.45, velocity 70.0, …, `blackboard.rs:285-320`). For the honest path, a missing field must become an explicit neutral/`None`, **not** an invented value — otherwise an honest-null pulse is silently "completed" with fabricated signal, defeating §3's principle.
2. **Score real signal only.** With real pulses arriving and defaults removed, the Evaluator's existing rubric machinery scores reality unchanged.
3. **Gate the fully-synthetic injectors.** `stress_event` (`leader.rs:1519`), the per-round synthetic `TraceEvent` (`leader.rs:1718`), and the `captain-async-planner` background ingest (`leader.rs:908`) become conditional on an explicit `inject_stress: bool` (CLI `--inject-stress`) that loads a **seeded scenario file** (`fixtures/stress-scenarios/baseline.json`) of recorded adverse `TraceEvent`s. Default off → clean path.
4. **Fix the real-base TUI site without blanking it.** At `leader.rs:1607` the `scores` are the *real* arena scores plus a `sin()/cos()*0.02` jitter, sent alongside genuinely-synthetic `lock_states`/`crdt_sync_frequency`/`conflicts_count`; `korg-tui` consumes this exact variant for the persona dashboard (`korg-tui/lib.rs:2588`). **Drop only the jitter** (send raw real scores) and gate/zero just the synthetic sub-fields — do **not** gate the whole `try_send`, which would empty the real panel.
5. **Ledger attests facts.**
   - `mutations_this_round` is an **incoming parameter** of `persist_campaign_ktrans` computed at the **call site** `leader.rs:1743` (`arena_outcome["mutations"].as_u64().unwrap_or(3) + round%2`). Fix it **there**, consuming the real numstat plumbed up via `PersonaResult`. Once real, `blast_radius` (`:687`, already a function of `mutations_this_round`) becomes real for free.
   - `total_mutations_so_far` (`:680`, `(round+1)*5`) is inside the body but inside a `spawn_blocking` closure — thread a real running counter on the orchestrator (`self`) into that closure.
6. **Re-tune rubrics if needed.** `RubricConfig` thresholds were tuned to the synthetic magnitudes; document and, if the real distribution requires, re-tune so the critic still fires (a non-compiling patch must trip `risk_score`). Covered by Unit C tests.

## 5. Data flow (end-to-end, default hermetic path)

```
korg campaign --headless "<fixture task>"
  └─ LeaderOrchestrator::run_observable_campaign_internal (leader.rs:1352)  [leader process]
       └─ dispatch_concurrent → spawn `korg worker` (Benjamin)              [child process]
            └─ run_task_in_worktree (harness.rs:521):
                 run_persona → DeterministicProvider.complete() → patch JSON (+ real usage)
                 APPLY patch to worktree → git diff --numstat → 3-state cargo check
                 → pack real fields into per_agent JSON; attach numstat to result
            └─ emit AcpMessage::SwarmTelemetryPulse{ per_agent: real } ──ACP stdio──►
       └─ drain real pulses → metrics_to_trace_event (no invented defaults) → Evaluator.evaluate()
       └─ persist_campaign_ktrans{ mutations_this_round = real numstat file count, … }  → signed .ktrans
  └─ korg verify  → chain valid AND attested mutations == real diff file count  ✅ TRUE
```

## 6. Error handling

- **Patch fails to apply** → `conflict_rate` reflects reject proportion, `risk_score` high; recorded honestly, worker continues. Never swallowed.
- **`cargo check` Failed** → a real signal (high risk, low confidence), not an error to suppress.
- **`cargo`/`git` absent** (degraded host) → the 3-state helper returns `Unavailable` → `verified_count_delta = 0` + a recorded `tool_unavailable` note (distinct from "passed"). Never a fabricated value.
- **Honest-null cognition** → 0 mutations, low confidence — a valid attestable outcome.
- **`sysinfo` probe error** → `gpu_util = 0.0`, documented (the crate is compiled in; this is an API-error fallback, not an "absent" case).
- The worker subprocess boundary already isolates failures; SP1 preserves "a crashed/aborted worker is a visible record, not an omission."

## 7. Testing

Hermetic (no network, no secrets), CI-gateable:
1. **Unit A:** `DeterministicProvider.complete()` is pure — same `(role, task, seed)` → byte-identical text; fixture task → applyable patch; unknown task → honest-null; truthful token usage; role correctly recovered from the system-message marker.
2. **Unit B (in worker):** given a known patch + fixture worktree, the emitted `per_agent` keys equal measured reality — a patch adding 1 file / 3 lines → numstat-derived blast radius matches; a non-compiling patch → `risk_score` high, `verified_count_delta = 0`; the 3-state cargo helper distinguishes Passed / Failed / Unavailable.
3. **Unit C:** `metrics_to_trace_event` no longer invents defaults (a missing field → neutral/None, asserted); with `--inject-stress` **off**, no scored `TraceEvent` has `agent_id == "stress-test-worker"` and none is sin-derived; with it **on**, the seeded scenario loads; the `:1607` panel still receives real scores (non-empty) with no jitter.
4. **End-to-end (keystone):** `korg campaign` on `fixtures/honest-demo-repo/` produces a `.ktrans` that `verify_chain` accepts **and** whose attested `mutations_this_round` equals the real `git diff --numstat` file count. *(Assert this verifiable invariant — not byte-identical ledger bytes, since the ledger stamps real `Uuid::now_v7()` / `Utc::now()`.)*
5. **Degraded host:** with `cargo` forced absent, the pipeline completes with `tool_unavailable` telemetry, no panic.
6. **Regression guard:** existing `MockProvider` tests pass unchanged; `test_playhead_steering_fork_campaign_reset` (`leader.rs:3853`, which relies on the mock default + runs the full campaign/persist path) is updated to the new default/flow; the other 7 in-file `#[tokio::test]`s (arena/merkle/healing sub-units) are checked.

## 8. Integration points (concrete, verified)

| Area | File:line | Change | Kind |
|---|---|---|---|
| Provider trait | `korg-llm/lib.rs:158` | add `DeterministicProvider` impl | net-new |
| Provider routing | `korg-llm/lib.rs:2104` (`build_provider_with` match) | add `"deterministic" =>` arm (wildcard is `MockProvider`) | net-new |
| Default provider | `korg-llm/lib.rs:1853` **and** `:2051` | flip `"mock"` → `"deterministic"` (both sites) | edit |
| Benjamin schema | `Prompts/benjamin.md:18-28` | add applyable `content`/`patch` field | edit |
| Worker body | `korg-runtime/harness.rs:521` (`run_task_in_worktree`) | insert apply+measure between `:530` and `:552`; runs in child | net-new |
| Persona fn (ref only) | `korg-runtime/personas.rs:465` (`run_persona`) | add token `usage` to `PersonaResult` (`personas.rs:62`) | edit |
| Apply patch | worker body (above) | `git apply`/file-write helper (no existing API) | net-new |
| Numstat | worker body | `git diff --numstat` helper | net-new |
| Cargo check | `korg-runtime/workers.rs:831` | new 3-state helper, `pub(crate)`, callable from harness | net-new |
| Pulse emit | `korg-runtime/harness.rs:319-388` | pack real fields into `per_agent` JSON of `AcpMessage::SwarmTelemetryPulse` (`acp.rs:351`) | edit |
| Numstat plumbing | `korg-runtime/workers.rs:766` (`spawn_worker_process`) | surface worker numstat onto `PersonaResult` | net-new |
| Trace defaults | `korg-runtime/blackboard.rs:285-320` (`metrics_to_trace_event`) | remove invented `unwrap_or` defaults (honest neutral/None) | edit |
| Synthetic gate | `leader.rs:1519`, `:1718`, `:908` | gate behind `--inject-stress` + seeded scenario | edit |
| TUI jitter | `leader.rs:1607` | drop sin/cos jitter (keep real scores); gate synthetic sub-fields | edit |
| Ledger mutations | `leader.rs:1743` (call site, **not** `:612` body) | consume real numstat instead of `unwrap_or(3)+round%2` | edit |
| Ledger running total | `leader.rs:680` (`spawn_blocking` closure) | thread a real running counter on `self` | edit |
| Dead code | `korg-runtime/harness.rs:581` (`build_live_evolving_pulse`) | delete | edit |
| sysinfo dep | `korg-runtime/Cargo.toml` | add `sysinfo` crate | net-new |
| Fixture | `fixtures/honest-demo-repo/` | minimal cargo crate + known bug + canonical patch | net-new |
| Scenario | `fixtures/stress-scenarios/baseline.json` | recorded adverse traces for `--inject-stress` | net-new |

## 9. Risks & blast radius

- **`leader.rs` is 3,872 lines and the single hub** — ~8 call sites from `main.rs`, plus `korg-server` and `korg-tui` consume its `TuiUpdate` stream. The `:1607` fix is the sensitive one (keep real scores, drop jitter) — handled per §4 Unit C #4 so the dashboard stays populated.
- **Cross-subprocess plumbing is a discrete task, not a field swap.** Worker (child) measures → puts numstat in its result/pulse → `spawn_worker_process` (`workers.rs:766`) surfaces it on `PersonaResult` → leader consumes at `leader.rs:1743`. Token usage likewise must be measured child-side (the leader's `CAMPAIGN_TOKENS` atomic is a different process).
- **`.ktrans` schema is consumed by `korg-verify` + the in-browser verifier.** Field *values* change; the **schema stays backward-compatible** with `korg-ledger@v1` — no field removals.
- **Provenance hashes payloads** — changing attested content changes recorded hashes, so any golden/example ledger (`examples/`, the linked session ledger) regenerates. Deliberate step.
- **New crate dependency** (`sysinfo`) added to `korg-runtime` (+ its transitive tree).
- **Default-provider flip is inert without the match arm** (`lib.rs:2104`) and the second default site (`lib.rs:1853`) — both must land with the flip, or output silently stays mock.
- **Rubric thresholds were tuned to synthetic magnitudes** — real data may need re-tuning so the critic still fires; covered by Unit C tests.
- **Test coupling:** 8 in-file `#[tokio::test]`s in `leader.rs`; campaign-level coupling is concentrated in `test_playhead_steering_fork_campaign_reset` (`:3853`), which assumes the mock default — update it. `MockProvider` itself is kept intact, limiting the rest of the test blast radius.

## 10. Out of scope / future (named for continuity)
- **SP2** — real collaboration: thread upstream persona outputs into downstream payloads (Captain→Benjamin→Lucas); per-persona permissions (Benjamin writes, Harper read-only). This is also where the apply+measure pattern generalizes from Benjamin to all five.
- **SP3** — real warm boot: wire the orphaned `SandboxPool` into `SpeculativeScheduler`; gate behind `--speculative`.
- **SP4** — honest headline demo: record the real binary on the fixture (using SP1's hermetic core) + real `korg rewind`; drop or build `korg fork`; fix README alt text.
