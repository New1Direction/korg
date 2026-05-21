# Korg Vault Log

Append-only chronological record of all significant writes and structural changes.

---

## 2026-05-19 — Vault Bootstrap (Part 2)

- Created `_GROK.md` (full operating manual with AI-first rules, propagation, folder map)
- Initialized `log.md`
- Restructured into wiki-style (`wiki/entities/`, `wiki/concepts/`, `wiki/projects/`, `wiki/daily/`, `wiki/sessions/`, `wiki/decisions/`, `wiki/synthesis/`, `raw/`, `research/`, `prompts/`)
- Migrated 0001 Project Kickoff decision into `wiki/decisions/`
- Created starter `wiki/projects/Korg.md`
- Updated `Index.md` to reference new structure and `_GROK.md`

---

## 2026-05-19 — Initial Structure + First Real Tooling

- `_GROK.md` written (core principles + folder map + propagation rules)
- `log.md` initialized
- First decision note moved into `wiki/decisions/`

- First successful run of `/korg-challenge` (self-correction on over-structuring risk)
- Decision recorded: adopt lighter pragmatic AI-first approach for early phase (see decision 0002)
- `korg-challenge` skill created and registered
- First cut of `korg-save` skill created and registered (updated with operational intelligence priorities)
- First daily note created (`wiki/daily/2026-05-19.md`)
- Major project note (`wiki/projects/Korg.md`) updated with activity
- Captured core semantic decision: Korg as Operational Intelligence Layer (not documentation)
- Captured long-term pattern vision: Cross-Harness Pattern Extraction
- First real application of the new mandate via updated `korg-save`

## 2026-05-20 — Harness-Agnostic State Primitives (Cross-Harness Pivot)

- Created `wiki/mechanisms/state-primitives.md` — canonical reference formalizing Epistemic State Machine (OBSERVED→INFERRED→VERIFIED→CONTESTED with deterministic VERIFIED criteria), Non-Interactive Doom-Loop Detection (token-velocity + AST delta >0.98 → SIGKILL), and Headless Merge-Arbitration Engine (authority-vector matrices).
- Tactical confirmation: Prioritized Option 4 (Cross-Harness Pattern Extraction) + preparation for Human/Methodology layer over further TUI-narrative work on Synthesis Failure Modes.
- New category `wiki/mechanisms/` introduced for pure, harness-agnostic Execution Primitive Modules.
- Full propagation: project note, new daily 2026-05-20, Index.md, Cross-Harness-Pattern-Extraction.md, and this log.
- This is the first concrete deliverable proving Korg scales beyond the Grok Build TUI to headless workers, pipelines, and swarms.

## 2026-05-20 — Leader-Broker Routing & Worktree Isolation Contracts (Concurrency Layer)

- Created `wiki/mechanisms/isolation-routing.md` — second core primitive: Leader-Broker Routing Protocol (capability mapping, epoch-window ack, STALLED status + routing failure artifacts), Ephemeral Worktree Isolation (cryptographically isolated `/tmp/korg/worktrees/$WORKER_ID`, read-only verified snapshots, single `.ktrans` transaction output), and Bounded-Buffer Back-Pressure (Drop-Oldest for INFERRED telemetry vs. hard Block + Leader notification for VERIFIED/CONTESTED candidates).
- Performed internal `korg-challenge` on the decision to extract immediately. Key hardenings applied: explicit mapping of routing events into the Epistemic State Machine, deferral of detailed `.ktrans` schema, positioning as contract rather than code, and acknowledgment of acceleration relative to the lighter pragmatic regime (decision 0002).
- Strong alignment noted with Decision 0001 (isolation to prevent cross-contamination) and the forward references already present in state-primitives.md.
- Updated daily/2026-05-20 with full challenge report and cross-links. Updated state-primitives Related section, Cross-Harness vision note, project note, Index, and this log.
- The two mechanism notes together now define a complete, self-consistent foundation for safe concurrent headless execution against the Korg state layer.

## 2026-05-20 — Transactional Memory Serialization Contract (Triad Completion)

- Created `wiki/mechanisms/transactional-memory.md` — the third and final foundational pillar.
  - `.ktrans` Transaction Anatomy: precise schema contract (`tx_id` as UUIDv7, `provenance_chain`, `mutations` array with `target_path` + `action` ∈ {INSERT, UPDATE, CONTEST}, authority_vector).
  - Decoupling Code Rollback from Memory Persistence: workers must emit flushable micro-`.ktrans` on any termination path, including doom-loop SIGKILLs. Memory survives execution death.
  - Three-Way Merge & Rebasement Protocol: Broker performs headless semantic rebase on stale-base late arrivals; higher-authority wins, conflicts produce first-class `semantic-decision` + `CONTESTED` artifacts.
- Performed internal `korg-challenge` before extraction (second challenge of the session). Addressed: schema satisfaction of prior promises, CONTEST actions flowing through (not bypassing) Merge-Arbitration, explicit cross-triad consistency rules.
- Updated all three mechanism notes with bidirectional links and consistency sections.
- Updated daily (with full challenge history), project note (triad completion milestone), log, Index, and Cross-Harness-Pattern-Extraction.md.
- The Korg headless runtime specification now rests on three mutually reinforcing, harness-agnostic contracts. The "what" (epistemic states), the "where/how" (isolation & routing), and the "what data moves" (`.ktrans` + rebasement) are fully specified.

## 2026-05-20 — First Human/Methodology/ Narrative (The Korg Triad)

- Created `Human/Methodology/The-Korg-Triad.md` — the opening piece of the human-facing layer.
  - Narrative, operator-focused explanation of the complete triad as a single coherent system.
  - Covers the "why" behind each pillar, a realistic worker lifecycle walkthrough (including death and survival of findings), key guarantees, and the experience of building harnesses against the contracts.
  - Deliberately written in a clear, thoughtful, conversational tone (distinct from the dense wiki/mechanisms/ specs).
- Updated project note, daily, Index, and `_GROK.md` to register the new Human/ layer.
- Added cross-links from all three mechanism notes to the new narrative (and vice versa) to maintain dual-layer connectivity.
- This completes the "technical foundation first, then story" sequence for the initial publishable core of Korg.

## 2026-05-20 — Building Your First Harness Against the Kernel (Human Layer Expansion)

- Created `Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md`
  - Practical guidance on the minimal surface every Korg harness must implement (worktree lifecycle, micro + terminal .ktrans emission, STALLED handling, provenance, clean termination).
  - Contrasts two patterns: Single-worker TUI/CLI driver vs. full Leader-Broker multi-worker swarm.
  - Includes “First 30 Minutes” compliance checklist and a pitfalls table showing exactly how the triad protects implementers.
- Full propagation completed (project, daily, log, cross-links from The-Korg-Triad.md and the three mechanism notes).
- Quick korg-challenge scheduled immediately after creation to verify consistency with the triad and prior narrative.

## 2026-05-20 — How to Watch a Live Campaign (Human Layer Trilogy Complete)

- Created `Human/Methodology/How-to-Watch-a-Live-Campaign.md`
  - Third piece in the initial Human/Methodology/ arc: understanding the kernel → building harnesses → operating live campaigns.
  - Focuses on real-time observability: the signals that matter (velocity/delta, .ktrans quality, STALLED, contest pressure, worker death patterns) and how to interpret them.
  - Includes healthy vs. troubled campaign patterns and clear guidance on when an operator should intervene versus trusting the kernel’s automated arbitration and doom-loop handling.
- Full propagation and cross-linking completed across the three human notes and the technical triad.
- This note completes a clean “understand → build → watch/operate” foundation for the human layer.

## 2026-05-20 — Reviewing and Resolving Contested Facts (Human Layer Foundation Complete)

- Created `Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md`
  - Fourth piece in the human layer: turns the `CONTESTED` state and Merge-Arbitration Engine into practical operator workflow.
  - Covers what contested facts are, why they exist, triage by authority differential, review workflow, common patterns, and clear criteria for when to create an explicit human `semantic-decision`.
  - Maintains consistency with the previous three human notes and the technical triad (especially state-primitives.md).
- Full propagation and cross-linking completed.
- The human layer now offers a complete “Understand → Build → Operate → Resolve” foundation before shifting focus toward reference implementations.

## 2026-05-20 — ACP Binding Design (Reference Implementation Layer Begins)

- Created `wiki/reference-harness/ACP-Binding-Design.md`
  - First design note in the new `reference-harness/` category.
  - Defines the minimal ACP message types required to make the three core contracts (Epistemic State Machine, Leader-Broker Routing & Isolation, Transactional Memory) work across distributed components.
  - Establishes core roles (Leader, Broker, Worker), message categories, `.ktrans` wire handling, and key lifecycle flows including doom-loop termination.
  - Language-agnostic and deliberately minimal.
- Updated `_GROK.md`, project note, daily, and Index to register the new folder and artifact.
- This is the deliberate shift from “foundation + explanation” into “how we actually make different harnesses interoperate.”

## 2026-05-20 — Leader-Broker ACP Model for Parallel Agents (Reference Layer Expansion)

- Created `wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md`
  - Directly builds on ACP-Binding-Design.md by showing how the Leader and Broker roles use the minimal ACP messages at scale.
  - Details work dispatch, STALLED handling, transaction/rebase flows, doom-loop termination coordination, and how the model supports both single-worker TUI and true multi-worker swarm styles.
  - Resolves the long-standing forward reference to “Leader-Broker-ACP-Model-for-Parallel-Agents” that appeared throughout the mechanism and human layers.
- Full propagation completed (project, daily, cross-links from ACP-Binding-Design and relevant human/mechanism notes).
- The reference-harness layer now has a clean two-note foundation (protocol + coordination model) before deeper schema or implementation work.

## 2026-05-20 — ACP Message Schema (Reference Layer Complete)

- Created `wiki/reference-harness/ACP-Message-Schema.md`
  - Third and final note in the initial reference-harness triad.
  - Provides exact message schemas for all ACP types, dual wire formats (JSON + CBOR), content hashing, provenance encoding, error taxonomy, size/frag rules, and versioning/extension guidelines.
  - Makes the ACP directly implementable while staying faithful to the triad contracts.
- Full propagation and cross-linking completed.
- The reference layer now offers a complete “protocol → model → schema” foundation ready for implementation work.

## 2026-05-20 — SuperGrok Heavy Multi-Agent Workflows (Pattern Extraction)

- Created `wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md`
  - Extracted the SuperGrok Heavy guide (personas, workflows, doom-loop recovery, monitoring checklist) as a reusable Korg pattern.
  - Includes a high-fidelity mapping table to the triad, the Leader-Broker-ACP model, and the human methodology layer.
  - Represents one of the strongest external real-world validations of Korg’s core primitives to date.
- Full propagation and cross-linking completed.
- This is the first significant external pattern captured under the Cross-Harness Pattern Extraction program.

## 2026-05-20 — Grok Build CLI Internals & Reference Layer Enrichment

- Created `wiki/reference-harness/Grok-Build-CLI-Internals.md` with detailed mechanics on worktree isolation, tool proxying, plan/review/approve loops, and CLI as ACP client.
- Significantly enriched `Grok-4.20-Heavy-Leader-Process-and-ACP.md` with Arena Mode pseudocode, DAG pruning logic, blackboard concurrency details, capability model, and production-grade headless/observability sections.
- These updates bring the reference-harness layer to a new level of fidelity with the actual Grok 4.20 Heavy production system. The ground-truth note now includes the full epistemic state machine with transition pseudocode, blackboard merge logic, DAG pruning, Arena details, capability model, Serving Infrastructure (Paged KV, Continuous Batching, Prefix Caching), and Token-Bucket Throttling formula + pseudocode.

## 2026-05-20 — Minimal ACP Client Pseudocode (Reference Implementation Sketch)

- Created `wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md`
  - First practical pseudocode harness that speaks the ACP defined in the Grok 4.20 Heavy architecture.
  - Demonstrates worktree isolation, tool proxying, blackboard interaction, .ktrans emission, Arena participation, and basic Leader + multi-worker coordination.
  - Includes clear mappings to the triad contracts and extension points toward full Grok Build-style workflows.
- This moves the reference-harness layer from specification to “buildable artifact.”

## 2026-05-20 — Minimal ACP Client Pseudocode — Complete End-to-End Reference (Major Expansion)

- **Major expansion** of `wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md`:
  - Added full `SessionManager` (create/resume, checkpointing, transaction replay, pending-work rehydration, recovery on disconnect/crash).
  - Added complete `LeaderOrchestrator.run_full_campaign` covering all five phases with rich detail: plan presentation + iterative user approval, parallel worktree dispatch with authority vectors, Arena participation + self-scoring vectors + hybrid synthesis, explicit `task.approve`/`task.reject` JSON payloads at both plan and final-synthesis gates, semantic three-way merge, and mandatory final `.ktrans`.
  - Added realistic `FullWorktreeWorker` with micro-transaction streaming + mandatory terminal `.ktrans` on every exit path (graceful, panic, SIGKILL).
  - Added Arena self-scoring + weighted aggregation matching the Merge-Arbitration Engine contract.
  - Added full recovery flow with re-dispatch and re-attachment semantics.
- The note is now the single most actionable, buildable artifact in the entire reference-harness layer — directly usable as a blueprint for a real Grok Build-style or Korg-compliant harness.
- This fulfills the recommendation to turn “excellent documentation” into “something people can actually build against.”

## 2026-05-20 — Anthropic Long-Running Agent Harnesses (Cross-Harness Pattern)

- Created `wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md`
  - Full pattern extraction from Anthropic Applied AI team talk (May 2026).
  - Documents Generator + Evaluator adversarial loops, Planner + Generator + Evaluator roles, persistent artifacts, verification loops, and agent team coordination.
  - Includes a detailed mappings table showing extremely strong alignment with Korg’s Arena Mode, transactional memory, Leader-Broker-ACP model, and Epistemic State Machine.
  - Added bidirectional cross-links to the ground-truth architecture note, the pseudocode harness, the 16-agent operator guide, and the core mechanism triad.
  - This extraction provides strong external validation for the design direction of the reference-harness layer and the Rust implementation work.

- Major step: The `LeaderOrchestrator` now spawns real child processes for each persona.
- New behavior in `leader.rs`:
  - `spawn_and_run_persona_worker()` uses `tokio::process::Command` + the current binary's `worker` subcommand.
  - Each persona (Captain, Harper, Benjamin, Lucas) runs in its own process with stdio JSON-line communication.
  - Leader sends `RouteWork`, reads `SubmitTransaction` + `TerminationReport`, then collects results.
  - The worker harness was adjusted to process one task and exit cleanly (perfect for short-lived spawned workers).
- `cargo run -- leader` now demonstrates the **Heavy-Adversarial Hybrid**:
  - Explicit contract negotiation step between Planner (Captain) and Evaluator before any work begins.
  - Contract is stored as a first-class artifact in the blackboard and on disk (referencable by .ktrans).
  - Evaluator persona integrated into the flow for harsh, contract-based verification.
  - This brings the Rust skeleton in line with the `Heavy-Adversarial-Hybrid-Harness` pattern.

- **Rust ACP module evolving toward Grok-native v1.17**:
  - `src/acp.rs` refactored with `MessageEnvelope<P>`, proper JCS canonicalization path, Ed25519 signing/verification helpers, and expanded error taxonomy (`AcpError`) with `state_invalidation` guidance.
  - Added payload structs for `PlanPresentation`, `TaskApprove`, `ArenaResult`, `ConflictResolve`, and `ToolInvoke`.
  - This is the first concrete step toward full spec compliance in the reference implementation. Other modules (Leader, harness) will be migrated incrementally.

- Created `wiki/patterns/Evaluation-Guardrail-Layer.md`
  - New focused pattern note for the Evaluation & Guardrail Layer.
  - Documents the five binary grading dimensions (with exact rubrics) and the semantic entropy + velocity doom-loop heuristics, including the productive-death differentiator.
  - Provides the authoritative reference for implementing the harsh Evaluator persona and guardrails in the Rust skeleton and for long-running Heavy Tier operation.
  - New `Evaluator` persona added (harsh critic with rubric scoring and "needs_revision" verdicts).
  - After a Generator (Benjamin) finishes, the Leader can spawn the Evaluator for live adversarial review.
  - This brings the executable harness in line with the strongest pattern from the Anthropic long-running agents extraction.
  - Leader loads `blackboard.json` on startup and passes the latest `last_snapshot` as `base_snapshot` to all workers.
  - Workers log the received base_snapshot (foundation for future rebasing).
  - After merge, `last_snapshot` is updated so subsequent campaigns (`--session` or `--resume`) build on previous state.
  - Added `--session <id>` / `--resume` CLI flags to demonstrate multi-campaign continuity.
  - Workers now write proper terminal `.ktrans` files (following the transactional-memory schema) on exit.
  - Leader reads the .ktrans files and performs a basic merge into `/tmp/korg/blackboard/blackboard.json` with provenance.
  - This brings the executable harness in line with the Korg transactional memory contract.
  - `prompt_plan_approval`: Shows the plan and accepts y/n/e/h with edit support.
  - `prompt_final_approval`: Shows ranked Arena results, accepts numbered choice, y/n/h/e.
  - The skeleton now gives a true "Grok Build / Heavy mode" feel with real user-in-the-loop gates.
- This brings the reference implementation much closer to how a real Grok Build-style harness would coordinate workers.

- Expanded `grok-acp-harness` with a complete in-process `LeaderOrchestrator` (as requested).
- New modules:
  - `src/personas.rs`: Clean implementations of Captain, Harper, Benjamin, and Lucas with self-scoring vectors matching the pseudocode.
  - `src/leader.rs`: `LeaderOrchestrator::run_full_campaign()` that executes the full flow:
    - Task decomposition into 4 persona-aware work packages
    - PlanPresentation + stubbed operator approval
    - Parallel (in-process) persona execution
    - Arena Mode with weighted aggregation of self-scores
    - ApprovalRequest + final `task.approve`/`task.reject` handling
    - Semantic merge stub + final campaign .ktrans
- Updated `harness.rs` to use the shared persona logic so worker mode and leader mode stay consistent.
- `cargo run -- leader` now produces a realistic end-to-end campaign trace.
- This brings the Rust skeleton to the point where it demonstrates the entire Grok Build-style loop described across the reference-harness layer and the Human/Methodology operator guide.

- Initialized a real, compilable Rust reference implementation at `reference-implementations/rust/grok-acp-harness/`.
- Structure:
  - `Cargo.toml` with tokio, serde, uuid, clap, anyhow, hex
  - `src/main.rs` — CLI entrypoint with `worker` and `leader` subcommands
  - `src/acp.rs` — ACP message types + `AcpClient` (directly modeled on the pseudocode `MinimalACPClient`)
  - `src/harness.rs` — `SingleWorkerHarness` implementing worktree lifecycle, task execution, micro/terminal `.ktrans`, and `TerminationReport`
- The skeleton follows the exact responsibilities and flow described in Sections 1–2 and 4.3 of `Minimal-ACP-Client-Pseudocode.md`.
- Currently demonstrates a complete worker lifecycle when run with `cargo run -- worker`.
- This marks the transition from “excellent pseudocode” to “runnable reference implementation” that people can actually clone and extend.

## 2026-05-20 — Minimal ACP Pseudocode — 4-Persona Specialization Pass

- Performed a focused expansion of `wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md`:
  - Added new **Section 6: Grok Build-Style 4-Persona Specialization (Captain, Harper, Benjamin, Lucas)**.
  - Includes concrete work-package decomposition, persona-aware dispatch, and explicit guidance on the thin-client surfaces an operator (following the new 16-agent watching guide) needs to see.
  - Strengthened cross-links to the freshly created `How-to-Watch-a-Live-16-Agent-Campaign.md`.
- This pass ensures the reference implementation sketch is fully aligned with both the ground-truth architecture and the human operator experience for real Heavy campaigns.

## 2026-05-20 — How to Watch a Live 16-Agent Campaign (Human Methodology Completion)

- Created `Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md`
  - New specialized operator guide focused on real Grok 4.20 Heavy / SuperGrok Heavy 16-agent campaigns.
  - Structures observation around the verified native 4-persona topology (Captain/Grok, Harper, Benjamin, Lucas) and how it scales.
  - Gives concrete, high-signal observables: token velocity vs. EffectiveBurnRate, Arena self-score vectors & aggregation, blackboard contention/rebasement pressure, PlanPresentation & ApprovalRequest events, worker lifecycle & productive death ratio, session recovery behavior.
  - Includes clear “Healthy 16-Agent Campaign vs. One Heading for Trouble” patterns and precise guidance on when and how to use the human gates (`task.approve`, `task.reject`, `capability.revoke`).
  - Positions itself as the Heavy-specific companion to the more abstract `How-to-Watch-a-Live-Campaign.md`.
  - Completes the core Human/Methodology “Understand → Build → Operate” arc using the full depth of the reference-harness layer.

## 2026-05-20 — Ground-Truth Enrichment with Verified 4-Agent Topology & KV Evidence

- Performed a targeted enrichment pass on `wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md` using newly confirmed primary-source details:
  - Native 4-agent persona topology (Grok/Captain, Harper, Benjamin, Lucas) running as specialized adapter heads on the shared ~3T MoE backbone (not separate models).
  - Marginal cost 1.5–2.5× (4-agent) thanks to weight + KV sharing; 16-agent mode via `agent_count=16` / `reasoning.effort=high|xhigh`.
  - Grok Build CLI launch timing (mid-May 2026) as the direct user-facing exposure of this swarm.
  - Real xAI API “Cached Input” pricing (~$0.20/M) as external validation of aggressive prefix/KV reuse.
- Strengthened the “Parallel Execution” and “KV Cache Strategy” sections with precise language and added a short “External validation” callout.
- Minor cross-link comment added to the expanded pseudocode harness for the common persona names (non-breaking).

## 2026-05-20 — Two Targeted Follow-on Notes (Reference Layer Completion)

- Created `wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md` — focused extraction of the dynamic token-bucket model, EffectiveBurnRate formula, rolling-window logic, velocity throttling, and soft-wall mechanisms.
- Created `wiki/reference-harness/Serving-Infrastructure-and-KV-Cache-Lifecycle.md` — focused extraction of Paged KV Cache, Continuous Batching, Prefix Caching, Expert Routing under concurrency, Multi-Runtime Isolation, KV Cache hydration/dehydration lifecycle, and topology-aware (zero-hop) scheduling.
- These keep the ground-truth note readable while allowing the two highest-density topics to grow independently if needed.

---

## 2026-05-21 — Full Project Audit + Rust Harness Compilation Patches & Verification

- Executed a complete codebase audit covering mechanisms, human operator narratives, ACP bindings, and the Rust reference skeleton.
- Created `code_review_and_audit.md` artifact summarizing project health, architectural constructs, and audit results.
- **Patched compile-breaking issue in `src/tools.rs`:** Added missing asynchronous `try_git_apply` process handler (which had missing reference blocking compilation with error `E0425`).
- **Fixed git-apply regression bug:** Refactored the patch writeback module to read the modified file from disk on success instead of overwriting the patched code back with the original unmodified content.
- Verified compilation and ran all unit tests (5 passed).
- Executed full multi-process `campaign` loop, demonstrating end-to-end child worker spawning (Captain, Harper, Benjamin, Lucas), CRDT Blackboard telemetry mapping, adversarial 5-rubric guardrails, dynamic swarm scaling (4 → 14), and signed `.ktrans` persistence.
- **Launched and verified Ratatui TUI Dashboard:** Executed `cargo run -- campaign --tui` as a background process to test Crossterm raw-mode initialization and interface frame rendering. Successfully validated the TUI launch sequences and event-loop thread integration before safely terminating the background task.

## 2026-05-21 — Resilient Worker Crash & Recovery Implementation

- **Extended `PersonaResult`** to support process health tracking (`crashed: bool` and `error_msg: Option<String>`).
- **Implemented deterministic crash simulation** in the worker `harness.rs`: when the RouteWork payload contains `"simulate-crash"`, the worker writes a partial `.ktrans` log containing intermediate mutations (e.g. to `src/auth.rs`) and exits with status 101.
- **Upgraded `LeaderOrchestrator` spawn and concurrent dispatch processes** to inspect the exit status of child worker processes.
- **Created a self-healing retry loop** in the Leader's `dispatch_concurrent`:
  - Detects worker process crashes.
  - Marks task packages as `STALLED`.
  - Scans and merges any partial on-disk `.ktrans` files into the central blackboard (clean rehydration).
  - Dynamically re-routes and re-spawns the work package to a fresh worker instance after stripping the `"simulate-crash"` tag.
- **Verified end-to-end recovery**: Ran campaign runs with `cargo run -- campaign`. Successfully verified the crash-and-recover loop live: Benjamin worker crashes, blackboard recovers the `src/auth.rs` changes, the leader re-spawns the task, and the campaign recovers perfectly to complete successfully.

## 2026-05-21 — Production Candle Embeddings for Real Semantic Entropy

- **Enabled `candle` feature by default in `Cargo.toml`**, transitioning the reference harness to production-grade semantic calculations out-of-the-box.
- **Wired real BERT (`all-MiniLM-L6-v2`) embeddings** into `src/evaluator.rs` and `src/embeddings.rs`.
- **Verified active local model loading** yielding `[Evaluator] Loaded real CandleEmbeddingModel (all-MiniLM-L6-v2)` logs on startup.
- **Confirmed authentic cosine-similarity calculations** for the five campaign rubrics, producing true dynamic semantic entropy ratings (`H_sem = 0.735`, `0.603`, `0.423`) during simulated multi-process campaigns.

## 2026-05-21 — Unified Diff Resiliency Implementation (Option 2)

- **Replaced naive patch application** in `src/tools.rs` with a robust, context-aware unified diff parser and applier.
- **Implemented Hunk AST:** Structured representations (`Hunk`, `HunkLine`, `HunkLineType`) mapping standard unified diff hunks.
- **Shift-Resilient Outward Search:** Finds target line indices dynamically by applying `cumulative_line_shift` and searching outward in alternating positive and negative offsets.
- **Multi-Stage Fuzzy Matcher:** Resolves differences in spacing, line endings, indentation, and case using 3 progressive stages.
- **Preserved Context Formatting:** Reconstructs context lines using original file content, protecting against casing and spacing corruption in applied blocks.
- **Headerless Raw Patch Fallback:** Automatically synthesizes single implicit hunks for patches lacking `@@` headers.
- **Verified with Tests & Campaigns:** Added 4 new comprehensive unit tests covering multi-hunk, line-shifting, fuzzy matching context preservation, and headerless diff applications. All 9 unit tests pass cleanly (`cargo test`). Verified the entire campaign orchestration compiles and finishes successfully under simulated swarms (`cargo run -- campaign`).

## 2026-05-21 — Evaluator Persona & Contract Negotiation Step Implementation

- **Implemented Real Semantic Similarity Method**: Added `score_similarity(&self, reference: &str, candidate: &str) -> f32` to `Evaluator` in `src/evaluator.rs` to compute local cosine similarity scores on demand.
- **Created Multi-Round Contract Negotiation Loop**: Developed a 3-round closed-loop protocol inside `negotiate_contract` in `src/leader.rs`. The Captain proposes criteria that the Evaluator critiques and rejects via cosine similarity until they pass the strict `0.42` similarity and length thresholds, with a robust forced-fallback in Round 3.
- **Enabled TUI Split-Screen Observability**: Refactored `draw_dashboard` in `src/tui.rs` to split the center panel, dedicating the top 45% to displaying the active Negotiated Contract with green checkbox bullet points and the bottom 55% to the Arena History.
- **Verified with Tests & Live Swarm Campaigns**: Added a comprehensive `test_contract_negotiation_loop` test in `src/leader.rs`. Verified that all 10 unit tests compile and pass cleanly, and that campaign runs execute the multi-round negotiation to completion.

## 2026-05-21 — Interactive TUI Gates for Swarm Contract Criteria Implementation (Option 1)

- **Established Bidirectional TUI-Campaign Feedback Channels**: Wired a custom `ContractResponse` channel (`feedback_tx`/`feedback_rx` via tokio mpsc) between the `KorgTui` event loop and the background `LeaderOrchestrator` campaign loop thread.
- **Created Swarm Contract Negotiation Modal**: Designed and implemented a high-visibility Ratatui popup overlay in `src/tui.rs` that renders Captain's proposed criteria, round details, and similarity scores using `Line::from` structures.
- **Supported Live Override Text Entry**: Enabled live typing buffers inside the TUI with backspace correction, escape-to-cancel, and enter-to-submit keybindings. Operators can now type custom overrides for contract criteria on the fly.
- **Implemented Interactive Gate in `negotiate_contract`**: Upgraded `src/leader.rs` to block contract negotiation until the TUI operator submits an action (`Approve`, `Reject`, `Force`, or `Override`), with a transparent automated fallback for headless execution.
- **Verified via Unit Tests & Campaign Runs**: Confirmed that all 10 unit tests compile and pass cleanly, including the new interactive gate logic. Tested headless and interactive workflows to ensure campaign continuity.

## 2026-05-21 — Top-Level CLI Command `korg <prompt>` Integration

- **Integrated Positional `prompt` and `--headless` Flag in `main.rs`**: Refactored the core Clap parsing logic in the Rust harness CLI to allow directly executing `korg "my task"` at the command line. Bypasses the Ratatui interactive dashboard when `--headless` is provided.
- **Upgraded `run_tui_with_campaign` signature**: Refactored `src/tui.rs` to accept `prompt: String`, dynamically spawning the campaign on the custom task instead of a hardcoded placeholder string.
- **Implemented Top-Level CLI Router**: Wired Clap parsing logic in `main()` to directly run a custom campaign either in Ratatui interactive mode or plain-log observable terminal output mode. Cleanly displays the Clap help manual when no arguments are provided.
- **Verified Complete Swarm Campaign Loop**: Ran comprehensive automated and manual verification. Verified that all 10 unit tests compile and pass cleanly. Successfully executed a headless swarm campaign (`cargo run -- "Refactor database pool with automatic retries" --headless`) executing all 5 phases: Contract Negotiation, concurrent Worker Dispatch, Resilient Recovery of Benjamin worker crash simulation, Evaluator semantic BERT scoring, and final signed `.ktrans` commits.

## 2026-05-21 — Premium Enhanced Ratatui TUI Dashboard & Upgraded Telemetry Pipeline

- **Refactored TUI Layout for Prevented Overlaps**: Redesigned `draw_dashboard` in `src/tui.rs` into a premium multi-grid command center dashboard, partitioning panels vertically and horizontally to prevent any text truncation or panel overlap under standard terminal heights.
- **Integrated Semantic Entropy Dials & History Sparklines**: Replaced simple text descriptors with a color-coded live `Gauge` for `H_sem` (green/yellow/red thresholds) and an inline `Sparkline` illustrating semantic entropy evolution history over time.
- **Added Per-Persona Confidence Vertical BarCharts**: Rendered a `BarChart` detailing individual confidence ratings for the four active Swarm Personas (Captain, Harper, Benjamin, Lucas).
- **Constructed Blackboard Contention & Locks Table**: Built a highly detailed telemetry `Table` showing active worker locks (READ/WRITE/IDLE), lock latencies, CRDT merge counts, conflict counters, and sync frequencies.
- **Enriched Swarm Contract Views with Inline BERT Similarity Scores**: Upgraded both `ContractNegotiated` lists and the active `ContractApprovalRequest` modal popup to render individual proposed criteria side-by-side with live BERT cosine-similarity scores in high-contrast styling.
- **Pipelined Dynamic Swarm and Campaign Health Telemetry**: Extended `LeaderOrchestrator` in `src/leader.rs` to compute dynamic similarity and campaign health parameters (velocity, risk, progress, doom_prob), pipelining them directly to the TUI event loop alongside periodic `PersonaTelemetry` lock updates.
- **Verified Complete Compilation & Clean Runs**: Confirmed that the entire suite of 10 unit tests compiles and passes flawlessly (`cargo test`), and verified successful execution loops in both standard interactive and headless mode.

## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 21 semantic contradictions, auto-resolved 5, and flagged 16 as unresolved conflicts.
- **Details:**
- Auto-resolved: [[Operational-Intelligence-Layer-Mandate]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Korg]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[AI-First Vault Principles]] (updated via [[state-primitives]])
- Auto-resolved: [[0001 - Project Kickoff]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[0002 - Lighter Pragmatic AI-First Approach for Korg]] (updated via [[AI-First Vault Principles]])
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Korg]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — state-primitives and transactional-memory]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — isolation-routing and transactional-memory]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 5 synthesis pages under `wiki/synthesis/`.
- **Details:**
- [[Synthesis — Semantic Entropy]] (connecting [[Korg]], [[Evaluation-Guardrail-Layer]])
- [[Synthesis — Blackboard]] (connecting [[Korg]], [[Evaluation-Guardrail-Layer]], [[Anthropic-Long-Running-Agent-Harnesses]])
- [[Synthesis — Transactional Memory]] (connecting [[Korg]], [[state-primitives]], [[isolation-routing]], [[transactional-memory]], [[Anthropic-Long-Running-Agent-Harnesses]], [[Conflict — isolation-routing and transactional-memory]], [[Conflict — state-primitives and transactional-memory]], [[Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]], [[Conflict — transactional-memory and Evaluation-Guardrail-Layer]], [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]])
- [[Synthesis — Evaluator Persona]] (connecting [[Korg]], [[Evaluation-Guardrail-Layer]])
- [[Synthesis — Adversarial Loop]] (connecting [[Korg]], [[Anthropic-Long-Running-Agent-Harnesses]])


## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 105 semantic contradictions, auto-resolved 24, and flagged 81 as unresolved conflicts.
- **Details:**
- Auto-resolved: [[isolation-routing]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Anthropic-Long-Running-Agent-Harnesses]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — state-primitives and isolation-routing]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — transactional-memory and Evaluation-Guardrail-Layer]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[Conflict — Korg and 0001 - Project Kickoff]] (updated via [[AI-First Vault Principles]])
- Auto-resolved: [[AI-First Vault Principles]] (updated via [[0002 - Lighter Pragmatic AI-First Approach for Korg]])
- Auto-resolved: [[SuperGrok-Heavy-Multi-Agent-Workflows]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — isolation-routing and transactional-memory]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — state-primitives and Cross-Harness-Pattern-Extraction]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — Operational-Intelligence-Layer-Mandate and Korg]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]] (updated via [[Operational-Intelligence-Layer-Mandate]])
- Auto-resolved: [[Conflict — state-primitives and transactional-memory]] (updated via [[Korg]])
- Auto-resolved: [[state-primitives]] (updated via [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]])
- Auto-resolved: [[transactional-memory]] (updated via [[Conflict — isolation-routing and transactional-memory]])
- Auto-resolved: [[Evaluation-Guardrail-Layer]] (updated via [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]])
- Auto-resolved: [[Cross-Harness-Pattern-Extraction]] (updated via [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]])
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — state-primitives and transactional-memory]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — isolation-routing and transactional-memory]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — Operational-Intelligence-Layer-Mandate and Korg]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — isolation-routing and transactional-memory and Conflict — Operational-Intelligence-Layer-Mandate and Korg]]
- Flagged: [[Conflict — Conflict — isolation-routing and transactional-memory and Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — isolation-routing and transactional-memory and Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — isolation-routing and transactional-memory and Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — Conflict — isolation-routing and transactional-memory and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — state-primitives and transactional-memory]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — Operational-Intelligence-Layer-Mandate and Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and Cross-Harness-Pattern-Extraction and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and Korg and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — state-primitives and isolation-routing]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — state-primitives and Evaluation-Guardrail-Layer and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — state-primitives and isolation-routing and Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Conflict — state-primitives and isolation-routing and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — state-primitives and isolation-routing and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — state-primitives and isolation-routing and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — state-primitives and isolation-routing and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — transactional-memory and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — transactional-memory and SuperGrok-Heavy-Multi-Agent-Workflows and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — transactional-memory and Evaluation-Guardrail-Layer and Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — transactional-memory and Evaluation-Guardrail-Layer and Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Conflict — transactional-memory and Evaluation-Guardrail-Layer and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — transactional-memory and Evaluation-Guardrail-Layer and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg and Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Conflict — Korg and 0001 - Project Kickoff and Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 0 synthesis pages under `wiki/synthesis/`.
- **Details:**
- No new synthesis pages created.


## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 31 semantic contradictions, auto-resolved 0, and flagged 31 as unresolved conflicts.
- **Details:**
- Flagged: [[Conflict — AI-First Vault Principles and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — AI-First Vault Principles and transactional-memory]]
- Flagged: [[Conflict — AI-First Vault Principles and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — AI-First Vault Principles and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — AI-First Vault Principles and 0001 - Project Kickoff]]
- Flagged: [[Conflict — AI-First Vault Principles and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and isolation-routing]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and transactional-memory]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — isolation-routing and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and 0001 - Project Kickoff]]
- Flagged: [[Conflict — transactional-memory and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — SuperGrok-Heavy-Multi-Agent-Workflows and 0001 - Project Kickoff]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 0 synthesis pages under `wiki/synthesis/`.
- **Details:**
- No new synthesis pages created.


## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 31 semantic contradictions, auto-resolved 0, and flagged 31 as unresolved conflicts.
- **Details:**
- Flagged: [[Conflict — AI-First Vault Principles and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — AI-First Vault Principles and transactional-memory]]
- Flagged: [[Conflict — AI-First Vault Principles and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — AI-First Vault Principles and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — AI-First Vault Principles and 0001 - Project Kickoff]]
- Flagged: [[Conflict — AI-First Vault Principles and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and isolation-routing]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and transactional-memory]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — isolation-routing and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and 0001 - Project Kickoff]]
- Flagged: [[Conflict — transactional-memory and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — SuperGrok-Heavy-Multi-Agent-Workflows and 0001 - Project Kickoff]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 0 synthesis pages under `wiki/synthesis/`.
- **Details:**
- No new synthesis pages created.


## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 37 semantic contradictions, auto-resolved 0, and flagged 37 as unresolved conflicts.
- **Details:**
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Korg]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and transactional-memory]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — AI-First Vault Principles and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — AI-First Vault Principles and transactional-memory]]
- Flagged: [[Conflict — AI-First Vault Principles and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — AI-First Vault Principles and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — AI-First Vault Principles and 0001 - Project Kickoff]]
- Flagged: [[Conflict — AI-First Vault Principles and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and isolation-routing]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and transactional-memory]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — isolation-routing and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and 0001 - Project Kickoff]]
- Flagged: [[Conflict — transactional-memory and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — SuperGrok-Heavy-Multi-Agent-Workflows and 0001 - Project Kickoff]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 0 synthesis pages under `wiki/synthesis/`.
- **Details:**
- No new synthesis pages created.


## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found 37 semantic contradictions, auto-resolved 0, and flagged 37 as unresolved conflicts.
- **Details:**
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Korg]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and transactional-memory]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Rust Async Performance Optimizations – Practical Investigation and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — AI-First Vault Principles and Operational-Intelligence-Layer-Mandate]]
- Flagged: [[Conflict — AI-First Vault Principles and transactional-memory]]
- Flagged: [[Conflict — AI-First Vault Principles and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — AI-First Vault Principles and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — AI-First Vault Principles and 0001 - Project Kickoff]]
- Flagged: [[Conflict — AI-First Vault Principles and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and isolation-routing]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and transactional-memory]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Operational-Intelligence-Layer-Mandate and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Korg and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Korg and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Korg and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — state-primitives and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — state-primitives and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — isolation-routing and Evaluation-Guardrail-Layer]]
- Flagged: [[Conflict — isolation-routing and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — isolation-routing and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — transactional-memory and Anthropic-Long-Running-Agent-Harnesses]]
- Flagged: [[Conflict — transactional-memory and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — transactional-memory and 0001 - Project Kickoff]]
- Flagged: [[Conflict — transactional-memory and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — Evaluation-Guardrail-Layer and SuperGrok-Heavy-Multi-Agent-Workflows]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and Cross-Harness-Pattern-Extraction]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0001 - Project Kickoff]]
- Flagged: [[Conflict — Anthropic-Long-Running-Agent-Harnesses and 0002 - Lighter Pragmatic AI-First Approach for Korg]]
- Flagged: [[Conflict — SuperGrok-Heavy-Multi-Agent-Workflows and 0001 - Project Kickoff]]
- Flagged: [[Conflict — 0001 - Project Kickoff and 0002 - Lighter Pragmatic AI-First Approach for Korg]]


## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created 0 synthesis pages under `wiki/synthesis/`.
- **Details:**
- No new synthesis pages created.


## 2026-05-21 — Full Project Audit & Competitive Roadmap Execution

- **Action:** Executed full repository and code audit of the Korg multi-agent harness (~6,534 LOC, 11 Rust modules).
- **Artifact Update:** Updated [[wiki/concepts/Korg-Audit-and-Competitive-Roadmap.md|Korg-Audit-and-Competitive-Roadmap]] with the complete, highly detailed audit results, competitive analysis matrices (for commercial systems like Grok Build, Codex CLI, Claude Code and open-source systems), and a structured 3-phase strategic roadmap (Phase 1: cognitive integration, Phase 2: security & DX, Phase 3: distributed execution & formal plan verification).
- **Visual Asset Integration:** Copied three generated high-contrast architectural diagrams and console mockups to `raw/screenshots/` and embedded them using a smooth Carousel layout inside the wiki concept note:
  - `architecture_diagram.png`: Decoupled Leader-Broker multi-persona topology.
  - `campaign_flowchart.png`: Closed-loop campaign transition phases.
  - `cli_ui_mockup.png`: Premium Crossterm/Ratatui dashboard cockpit.
- **Propagation:** Updated daily log, project goals, vault index, and internal wiki linking structures to maintain high-contrast conceptual integrity.


## 2026-05-21 — Korg Cockpit and Live Timeline Specification (ASEE Vision)

- **Action:** Created [[wiki/concepts/Live-Execution-Timeline-and-Cockpit-Spec|Live-Execution-Timeline-and-Cockpit-Spec]] mapping Korg's definitive product philosophy: building an autonomous runtime with an integrated editing surface.
- **Design Specifications**:
  - **Live Execution Timeline (Cognitive Git)**: Visual DAG layout for tracking signed JCS/Ed25519 `.ktrans` worker commits, critiques, and semantic merges.
  - **Replay Scrubber & Time-Travel Forking**: Interaction specifications for scrubbing backward, rehydrating Blackboard states via `RouteWork::Replay`, and forking executions into parallel branch swarms.
  - **Enterprise Policy Engine Primitives**: Zero-trust contract mapping allowlists, timeouts, and Evaluator-gated interrupts.
- **Propagation**: Linked from daily notes, `wiki/projects/Korg.md`, and added to the vault index.


## 2026-05-21 — Model-Agnostic LLM Provider Layer Implementation (Phase 1, Step 1)

- **Action:** Implemented the swappable cognitive abstraction core `src/llm.rs` along with all supporting Cargo dependencies (reqwest, futures-util, async-trait, bytes).
- **Core Engineering Primitives Established:**
  - **`LlmProvider` Trait**: Custom unified trait defining async complete and complete_stream functionality.
  - **Universal Types**: Normalized `Role`, `Message`, `LlmRequest`, `LlmResponse`, `LlmDelta`, `LlmError`, `TokenUsage`, `FinishReason`, `ToolDefinition`, `ToolCall`, `FunctionCall`.
  - **Lightweight Zero-SDK Clients**: Pure HTTP adapters with direct SSE chunk parsers mapping to `OpenAIProvider`, `AnthropicProvider` (handling top-level system parameter extraction and Claude Messages mapping), `GrokProvider` (xAI), and `LocalOllamaProvider`.
  - **MockProvider**: Fully stateful, thread-safe testing client using a deque queue of simulated results to ensure 100% offline unit-test stability.
  - **ResilientLlmProvider**: High-reliability decorator providing stateful exponential backoff retry cycles and a thread-safe custom `CircuitBreaker`.
- **Integrations & Verification:**
  - Registered `pub mod llm;` in `src/main.rs`.
  - Updated CLI Startup welcome banner inside `print_welcome_banner` to display active cognitive provider state dynamically.
  - Wrote 4 comprehensive tests inside `src/llm.rs` testing payload serializations, offline mock transactions, and retry logic.
  - Executed `cargo test` verifying all 16/16 unit tests compiled and passed cleanly.
- **Propagation:** Updated implementation plans, daily notes, project lists, and master logs. Created structural walkthrough artifacts.

