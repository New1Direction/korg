# Korg Swarm — SP2: Real collaboration (Track B)

**Status:** Design+plan / approved-by-delegation ("do it all now"), grounded against real code 2026-06-14
**Branch:** `feat/swarm-collaboration` (stacked on `feat/swarm-honest-demo`)
**Sub-project:** SP2 of Track B.

## 1. The honest problem (grounded)
- The DAG (`build_campaign_dag`) encodes the collaboration **graph** (benjamin depends on captain+harper; lucas on benjamin) but never passes **data** — `packages_map` is immutable and each `WorkPackage.description` is a static `"Plan/Research/Implement/Synthesize: {root_task}"` string. **Downstream personas never see upstream output.**
- Permissions are plumbed (`RouteWork.permissions`) but **ignored** (`_permissions`, hardcoded `fs:write:worktree` for everyone). No read-only/write distinction.
- Benjamin's worker is **deliberately crash-simulated**: `decompose_into_persona_packages` bakes `"Implement (simulate-crash)"` (leader.rs:2552) and `harness.rs:362` honors it → Benjamin never does real work in a campaign.
- `run_self_healing_loop` is a **no-op** (targets a worktree the worker child already deleted) and never re-plumbs `files_changed` after a heal.
- **Honesty constraint:** the default `DeterministicProvider` only emits real applyable content for Benjamin on the *fixture* task; every other (persona, task) returns honest-null. So collaboration plumbing alone yields empty downstream diffs offline. Real per-persona work offline requires a **role-aware** provider keyed on fixture-class tasks; arbitrary tasks need `--provider ollama`.

## 2. Goal
Make the collaboration **mechanism** genuinely real and verifiable: upstream persona output flows into downstream payloads, personas have real per-role behavior + permissions, the workers actually run (no fake crash), and the ledger count stays truthful through healing. Demonstrated deterministically on the **fixture task** (each persona emits a real role-shaped artifact); honest-null on tasks the stub can't do.

**Acid test:** in a campaign on the fixture task, (a) Benjamin's payload contains Captain's plan text (data-flow is real), (b) a read-only persona that emits mutations does NOT mutate the worktree (permissions enforced), (c) the workers complete without the fake crash, (d) the attested `mutations_this_round` equals the real summed diff including any heal.

## 3. Slices (each independently shippable + testable)

### Slice 1 — Role-aware DeterministicProvider + de-theater the workers
- Extend `korg-llm/src/deterministic.rs`: `role_marker` recognizes all 5 personas (Captain/Harper/Benjamin/Lucas/Evaluator via their prompt markers). For the **fixture-class task**, each emits a real, deterministic, role-shaped artifact:
  - Captain → `{work_packages, acceptance_criteria}` (a real plan referencing `src/lib.rs`),
  - Harper → `{concerns, risk_assessment}`,
  - Benjamin → the applyable patch (existing),
  - Lucas → `{resolutions}` (synthesis referencing the implement step),
  - Evaluator → `{passed_rubrics, recommended_action}`.
  - Any non-fixture task → honest-null per role (`[]`/empty + low confidence) — never fabricated.
- Remove the `(simulate-crash)` directive from `decompose_into_persona_packages` (leader.rs:2552) → `"Implement: {root_task}"`. (Leave the `simulate-crash` handler in harness for explicit fault-injection tests, but the default decomposition must not trigger it.)
- **Test:** each persona's provider output for the fixture task parses to a non-empty role-shaped artifact; non-fixture → honest-null. A campaign-level smoke test confirms Benjamin's worker no longer crash-simulates by default.

### Slice 2 — Real upstream→downstream data-flow
- In `dispatch_concurrent` (leader.rs): make `packages_map` mutable; after each level completes, rewrite each downstream node's `description` to append serialized upstream `PersonaResult.output` from its DAG dependencies (Captain's plan + Harper's concerns → Benjamin; Benjamin's output → Lucas). Carry it in the existing `RouteWork.payload` String (no ACP schema change), **size-capped** (reuse the 8000-char Heavy-Consciousness ceiling).
- **Test:** after L1, the benjamin node's payload contains Captain's plan marker; after Benjamin, lucas's payload contains Benjamin's output marker. (Mirror `test_build_campaign_dag_produces_four_levels`.)

### Slice 3 — Per-persona permissions + apply/analyze policy
- New `fn permissions_for(persona) -> Vec<String>` resolved at spawn (session.rs, replacing the hardcoded vec): Benjamin/Lucas → `fs:write:worktree`; Harper/Captain/Evaluator → `fs:read`.
- Stop ignoring `_permissions` in `handle_route_work`; in `run_task_in_worktree` gate the `apply_mutations` call on a write capability. A read-only persona that emits mutations → recorded as applied=0 (analyze-only), not mutating.
- **Test:** Harper (read-only) emitting a mutation yields `files_changed==0` and does not write the file; Benjamin (write) applies normally.

### Slice 4 — Self-healing re-plumb (follow-up #1)
- Fix `run_self_healing_loop`: heal in the worker child before worktree cleanup (or defer cleanup for heal-eligible nodes), then re-run `numstat` and update `files_changed` so the leader's `real_files_changed` sum (leader.rs:1484) reflects heals. Plumb the post-heal count via the existing `SubmitTransaction.files_changed`.
- **Test:** the existing `test_self_healing_loop_success` exercises a non-no-op heal and asserts the re-measured count flows through.

## 4. Autonomous decisions (per "do it all")
- **Carrier = the existing `payload` String** (thin, no ACP schema change), size-capped at 8000 chars.
- **Permission model = flat `Vec<String>` capability list**, derived from persona (matches the existing `RouteWork.permissions` shape).
- **Read-only violation response = analyze-only** (don't apply; record `files_changed=0`) + a logged note — NOT a hard task failure (keeps the campaign progressing honestly).
- **Lucas applies** the synthesized patch (gets `fs:write`); **Harper/Captain/Evaluator do not**.
- **Honesty boundary stated explicitly:** offline real work is demonstrated on fixture-class tasks via the role-aware stub; arbitrary tasks honestly honest-null offline and need `--provider ollama`. Do NOT claim "all personas do real work on any task."
- Keep upstream-context serialization deterministic/ordered (so the campaign Merkle root stays reproducible).

## 5. Verification
- `cargo test -p korg-llm` (role-aware provider tests) + `cargo test -p korg-runtime` (data-flow, permissions, self-healing) green.
- A campaign smoke test on the fixture task: workers complete (no fake crash), Benjamin's payload carries Captain's plan, the attested count reflects real diffs, permissions enforced.
- Full `cargo test --workspace` green; fmt + clippy clean.
- Honesty: a non-fixture campaign task still completes honestly with honest-null personas (attests what it really did, no fabrication).

## 6. Out of scope (later)
- Making arbitrary-task campaigns produce real work (needs a real model; `--provider ollama` is the path).
- SP3 (warm boot).
- The deeper worker-subprocess robustness (timeouts/idle) beyond removing the deliberate crash.
