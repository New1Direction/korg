---
title: "Korg"
date: 2026-05-21
type: concept
tags: [project, meta, korg, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---


## For future Grok

Korg is the isolated, AI-first knowledge base for the new project layer (the successor work after cli-anything and the early RedMicro experiments). This note tracks the project's own goals, architecture decisions, and status.

---

# Korg

**Status:** Active  
**Priority:** High  
**Started:** 2026-05-19

## Goal

Build a high-signal, agent-optimized second brain that compounds knowledge across long, multi-session Grok work without manual janitoring.

## Key Principles

- Wiki-style + AI-first note format
- Strong propagation discipline on every write
- Native Grok thinking tools (`korg-challenge`, `korg-reconcile`, etc.)
- Clean separation from personal knowledge graph (MINI)

## Current Focus Areas

- Establishing the vault foundation (`_GROK.md`, folder structure, log)
- Porting the highest-value patterns from obsidian-second-brain as Grok skills
- Defining the actual technical scope of the "Korg" project (api-anything successor layer, new harness patterns, etc.)

### Recent Activity (2026-05-19)

- First real run of `/korg-challenge` on the vault structure decision
- Explicit decision to start with a **lighter pragmatic AI-first approach** (see [[wiki/decisions/0002 - Lighter Pragmatic AI-First Approach for Korg]])
- Created and registered `korg-challenge` skill
- Created first cut of `korg-save` skill (now tuned for operational intelligence)
- Major philosophy clarification: Korg as **Operational Intelligence Layer** (optimize for retrieval/synthesis/pattern linking/semantic reuse)
- Captured high-value artifacts:
  - [[wiki/concepts/Operational-Intelligence-Layer-Mandate]] (semantic-decision)
  - [[wiki/patterns/Cross-Harness-Pattern-Extraction]] (pattern)
- Vault restructured to wiki-style with updated `_GROK.md`
- First daily note created

### Recent Activity (2026-05-20)

- **Major milestone:** Created [[wiki/mechanisms/state-primitives.md]] — first explicit Harness-Agnostic State Primitives module.
- Confirmed tactical direction: Cross-Harness Pattern Extraction (Option 4) coupled with Methodology pivot (Option 2). Prioritized de-TUI-ification of core mechanics (Epistemic State Machine, Non-Interactive Doom-Loop Detection, Headless Merge-Arbitration Engine).
- New `wiki/mechanisms/` category established for universal Execution Primitive Modules.
- **Second milestone:** Created [[wiki/mechanisms/isolation-routing.md]] — Leader-Broker Routing & Worktree Isolation Contracts (physical execution substrate, `.ktrans` handoff, differentiated back-pressure).
- **Third milestone (triad completion):** Created [[wiki/mechanisms/transactional-memory.md]] — Transactional Memory Serialization Contract. This defines the precise `.ktrans` anatomy (UUIDv7 `tx_id`, provenance_chain, mutations with INSERT/UPDATE/CONTEST actions), the decoupling of code rollback from memory persistence (doom-loop survival), and the headless Three-Way Merge & Rebasement Protocol for late-arriving transactions.
- Two internal `korg-challenge` reviews performed (one before each of the last two primitives). All findings incorporated. The three notes now form a closed, consistent specification.
- Full propagation completed across project, daily (with dual challenge reports), log, Index, Cross-Harness vision note, and bidirectional links between all three mechanism files.
- The architectural triad (epistemic states + physical isolation/routing + transactional handoff) is now fully specified and cross-linked. This is the minimal publishable foundation for Korg as a generalized semantic execution kernel.

### Human/Methodology/ Layer Progress (2026-05-20)

- Created `Human/Methodology/The-Korg-Triad.md` — first narrative explaining the triad as one closed operating system.
- Created `Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md` — second narrative (harness responsibilities and patterns).
- Created `Human/Methodology/How-to-Watch-a-Live-Campaign.md` — third narrative (monitoring and live operation).
- Created `Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md` — fourth narrative.
  - Completes a strong four-piece human foundation: Understand → Build → Watch/Operate → Review & Resolve.
  - Focuses on the `CONTESTED` state as the kernel’s epistemic immune system.
  - Details the operator review workflow, authority vector decision logic, common contest patterns, and when to escalate to explicit `semantic-decision` records.
  - Keeps the same clear, practical, operator-first voice.
- The human layer now provides a complete arc from understanding the contracts through building harnesses, watching campaigns, and handling the most important human-in-the-loop activity (contested fact resolution).

### Reference Harness / ACP Layer (2026-05-20)

- Created `wiki/reference-harness/ACP-Binding-Design.md` — first artifact in the new reference implementation layer (minimal ACP protocol).
- Created `wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md` — companion coordination model.
- Created `wiki/reference-harness/ACP-Message-Schema.md` — concrete message schemas and wire format.
  - Defines exact schemas, dual JSON/CBOR encoding, content hashing rules, error code taxonomy, size limits, fragmentation strategy, and high-level security considerations.
  - Completes a three-note reference foundation: protocol surface → coordination model → implementable schemas.
  - Remains language-agnostic while being precise enough for an implementer to begin coding.
- The reference-harness layer now provides a complete, validated, and concrete foundation for building interoperable Korg harnesses.

### Minimal Reference Implementation Sketch (2026-05-20)

- Created `wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md`
  - Practical pseudocode for a minimal ACP-speaking harness.
  - **Major expansion (2026-05-20)**: Converted into the primary buildable reference artifact — a complete, realistic Grok Build-style end-to-end workflow including:
    - `SessionManager` with full lifecycle, checkpointing, blackboard replay, and recovery on reconnect/crash.
    - `LeaderOrchestrator.run_full_campaign` with all five phases (plan presentation + approval, parallel worktree dispatch, Arena participation with self-scoring vectors, human `task.approve`/`task.reject` gates with rich payloads, semantic three-way merge + final `.ktrans`).
    - Detailed `FullWorktreeWorker` showing micro + mandatory terminal `.ktrans` emission on every exit path (including crashes).
    - Arena Mode self-scoring + weighted aggregation matching the Merge-Arbitration Engine.
    - Explicit JSON examples for `task.approve` (plan + final synthesis) and `task.reject`.
    - Recovery guarantees and re-dispatch logic.
  - Now one of the most useful artifacts in the vault for anyone implementing a Korg-compliant or Grok Build-style harness.
- This completes the reference-harness layer as "something people can actually build against."

### Grok Build CLI Internals & Supporting Details (2026-05-20)

- Created `wiki/reference-harness/Grok-Build-CLI-Internals.md` — dedicated extraction of Grok Build worktree management, tool proxying, plan/review/approve loops, and how the CLI acts as a thin ACP client.
- Enriched `Grok-4.20-Heavy-Leader-Process-and-ACP.md` with detailed sections on Arena Mode (with self-scoring pseudocode), Task DAG construction & pruning logic (with pseudocode), Blackboard concurrency & delta patching (including merge pseudocode), Capability & Permission Model (deep dive), Epistemic State Machine transitions (OBSERVED → INFERRED → VERIFIED → CONTESTED with transition handler), Serving Infrastructure (Paged KV, Continuous Batching, Prefix Caching), Token-Bucket Throttling formula + pseudocode, and expanded CLI/Headless usage.
- These updates incorporate the latest primary-source depth on scheduling, blackboard management, failure recovery (productive death), observability, security model, and cluster engineering from the real Grok 4.20 Heavy system.
- Additional ground-truth enrichment pass (same day) incorporated verified details on the native 4-agent persona topology (Captain/Grok, Harper, Benjamin, Lucas as adapter heads on the shared MoE), exact marginal cost numbers (1.5–2.5×), 16-agent configuration signals, Grok Build CLI launch date, and the xAI API Cached Input pricing signal as real-world proof of the KV cache architecture.

### Two Targeted Follow-on Notes (2026-05-20)

- Created `wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md` — focused extraction of the dynamic token-bucket model, EffectiveBurnRate formula, rolling-window logic, velocity throttling, and soft-wall mechanisms.
- Created `wiki/reference-harness/Serving-Infrastructure-and-KV-Cache-Lifecycle.md` — focused extraction of Paged KV Cache, Continuous Batching, Prefix Caching, Expert Routing under concurrency, Multi-Runtime Isolation, KV Cache hydration/dehydration lifecycle, and topology-aware (zero-hop) scheduling.
- These keep the ground-truth note readable while allowing the two highest-density topics to grow independently if needed.

### Human/Methodology Layer — 16-Agent Operator Guide (2026-05-20)

- Created `Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md`
  - Practical operator-first guide for watching real Grok 4.20 Heavy / 16-agent swarms.
  - Uses the newly confirmed 4-persona topology (Captain/Grok, Harper, Benjamin, Lucas) as the primary lens.
  - Covers token-bucket / EffectiveBurnRate signals, Arena self-score vectors, blackboard contention, PlanPresentation & ApprovalRequest events, productive death ratio, session recovery behavior, and clear “healthy vs. drifting into trouble” patterns.
  - Completes the “Understand → Build → Operate” arc started with the earlier Human/Methodology notes.
  - Explicitly cross-links the enriched ground-truth architecture and the expanded pseudocode harness.
- **Further expansion** of `Minimal-ACP-Client-Pseudocode.md`: Added dedicated Section 6 “Grok Build-Style 4-Persona Specialization” with concrete work-package examples for Captain, Harper, Benjamin, and Lucas, plus guidance on how the thin client should surface the exact signals the operator guide recommends watching. This makes the pseudocode note the single most complete buildable reference in the vault.
- **Real reference implementation**: Created `reference-implementations/rust/grok-acp-harness/`.
  - Real stdio transport + 4-persona dispatch (Captain, Harper, Benjamin, Lucas).
  - `LeaderOrchestrator` now spawns each persona as a **separate child process** (using the `worker` subcommand over stdio).
  - True multi-process orchestration: Leader sends `RouteWork`, receives results + termination reports from independent workers.
  - `cargo run -- leader` now runs a genuine multi-process version of the full campaign loop.

### ACP v1.17 Wire Format Reference (2026-05-20)

- Created `wiki/reference-harness/ACP-v1.17-Wire-Format.md`
  - Canonical condensation of the Antigravity Gemini 3.5 ACP v1.17 spec.
  - Covers JCS canonicalization, Ed25519 signatures, CRLF streaming, full message registry (including PlanPresentation, task.approve, ArenaResult, etc.), and the detailed error taxonomy.
  - Added Korg-specific commentary and mappings to the pseudocode, Rust skeleton, and Heavy-Adversarial pattern.

- Created `wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md`
  - Detailed cross-harness extraction from Anthropic’s May 2026 AI Engineer Conference talk on long-running agent systems.
  - Strong mappings to Korg’s Arena Mode, transactional memory, Leader-Broker-ACP model, and Epistemic State Machine.
  - Highlights the Generator + Evaluator adversarial loop, Planner + Generator + Evaluator roles, persistent artifacts, and the dangers of self-evaluation — all of which validate core design decisions in the reference-harness layer and the Rust skeleton.

- Created `wiki/patterns/Evaluation-Guardrail-Layer.md`
  - Dedicated pattern note for the Evaluation & Guardrail Layer.
  - Defines the five binary grading dimensions for the Evaluator persona (Trajectory Efficiency, Epistemic Integrity, Tool-Use Precision, Semantic Adherence, Resource Utilization) with exact Pass/Fail rubrics.
  - Details advanced doom-loop heuristics (dynamic token velocity + semantic entropy `H_sem`) and the productive-death vs. doom-loop differentiator.
  - Provides integration rules with contracts, ACP messages (`EvaluationVerdict`, `RequestTerminate`), Arena, and mandatory `.ktrans`.
  - Includes forward-looking hooks for the Rust skeleton implementation.

- Created `wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md`
  - High-value extraction of the SuperGrok Heavy guide as a first-class Korg pattern.
  - Captures personas with explicit output contracts, reusable workflow templates (Best-of-N + Selector, Parallel Implementers + Integrator, etc.), doom-loop detection & recovery strategies, and a monitoring checklist.
  - Includes a detailed mapping table showing how the pattern aligns with Korg primitives (doom-loop detection, worktree isolation + merge, Leader-Broker coordination, epistemic governance).
  - One of the strongest external validations of the entire Korg architecture to date.
- This is the first major external pattern extraction performed under the Cross-Harness mandate.

### Audit & Reference Skeleton Verification (2026-05-21)

- **Completed Full Project Audit:** Audited the mechanisms, reference schemas, human methodology guides, and executable Rust code. Documented comprehensive findings in a dedicated brain audit report: [[code_review_and_audit.md]].
- **Resolved Rust Crate Compilation Obstacles:** Fixed a compilation blocker in `src/tools.rs` where the `try_git_apply` function was referred to but not defined (error `E0425`). Also fixed a logic bug where a successful git apply patch would be immediately overwritten with the unmodified `original` file content.
- **Verified Crate Test Integrity:** Successfully ran and verified all unit tests (5 passed).
- **Campaign Execution Success:** Launched the full multi-process observable `campaign` loop over simulated child-process workers, validating live telemetry ingestion, 5-rubric Evaluation, dynamic swarm scaling (from 4 up to 14), and signed `.ktrans` persistence.
- **TUI Dashboard Verification:** Successfully launched `cargo run -- campaign --tui` as a background process to test the Crossterm-backed terminal raw-mode initialization and frame updates. The TUI event loop initialized cleanly alongside the background orchestrator and campaign thread.
- **Worker Crash & Recovery (Option 3) Implementation:** Implemented a robust worker fault resiliency loop. The leader now detects worker exit code crashes (such as simulated exits with code `101`), pauses progress to state the round as `STALLED`, scans the local `/tmp/korg/ktrans` directory to rehydrate the central blackboard from the worker's intermediate partial `.ktrans` log, cleanses the payload, and dynamically re-spawns a new worker child process. The recovery loop was successfully verified end-to-end using the Benjamin worker.
- **Candle Embeddings Integration (Option 1):** Enabled real semantic entropy calculations in the Evaluator persona by enabling the `"candle"` feature by default in `Cargo.toml`. The Evaluator now automatically loads the `all-MiniLM-L6-v2` model weights and tokenizer using Candle. Live campaigns correctly generate genuine, dynamic semantic entropy values (`H_sem`) based on cosine similarity of actual worker telemetry outputs, validating the complete closed-loop Heavy-Adversarial hybrid coordination model.
- **Unified Diff Resiliency (Option 2):** Upgraded `apply_simple_unified_diff` to a robust, context-aware, shift-resilient multi-hunk unified diff parser and applier. This features a structured representation (`Hunk`, `HunkLine`), standard header parsing, outward search centering around a cumulative-line-shift index, a multi-stage fuzzy matching fallback strategy (for trailing spaces, indentation, and case differences), original formatting preservation for context lines, and automatic synthesis of implicit hunks for headerless patches. Verified with 4 comprehensive new unit tests and a full campaign execution.
- **Evaluator Persona & Contract Negotiation (Option 1):** Closed the loop on the Heavy-Adversarial hybrid pattern by integrating a real, multi-round adversarial Contract Negotiation step. The Planner (`Captain`) and the `Evaluator` negotiate plan criteria over up to 3 rounds. The Evaluator performs real local BERT cosine-similarity checks against the task description using `score_similarity(&self, reference: &str, candidate: &str) -> f32` to reject generic criteria (average similarity $< 0.42$ or fewer than 3 criteria). The final signed contract is persisted to `/tmp/korg/contracts/` as a first-class versioned JSON file. Refactored the TUI dashboard center panel to show the Negotiated Contract (acceptance criteria) dynamically in the top 45% using green checkbox bullet points and the Arena History in the bottom 55%. Verified with a comprehensive unit test (`test_contract_negotiation_loop` in `src/leader.rs`) and live campaign execution loops.
- **Interactive TUI Gates (Option 1):** Closed the interactive steering loop on contract negotiations. Added bidirectional `feedback_tx`/`feedback_rx` tokio channels from the Crossterm-backed TUI event loop back to the `LeaderOrchestrator` campaign loop. Created a custom overlay modal popup in `src/tui.rs` that renders Captain's proposed criteria and similarity score in high-contrast yellow/bold. Implemented full backspace and character typing capture to support a live Operator Override mode. Modified `negotiate_contract` in `src/leader.rs` to block campaign progression until the TUI operator submits an action (`Approve`, `Reject`, `Force`, or `Override`), falling back cleanly to automated evaluation when in headless mode. Verified all changes pass the automated test suite perfectly (`cargo test`).
- **Top-Level CLI Entry Point Integration:** Implemented the positional `prompt` argument and `--headless` flag to allow running immediate campaign loops with a single CLI command (e.g., `korg "my task"`). Bypasses the TUI for scripting/CI via the `--headless` flag. Preserved all pre-existing subcommands with perfect backwards compatibility. Successfully verified end-to-end execution of contract negotiation, concurrency, resilient recovery, unified diff application, real BERT embeddings semantic evaluation, and signed `.ktrans` persistence.
- **Premium Enhanced Ratatui TUI Dashboard & Telemetry Pipeline:** Refactored `draw_dashboard` in `src/tui.rs` into a stunning multi-grid dashboard, partitioning panels vertically and horizontally to prevent terminal overlaps. Integrated color-coded live `Gauge` for semantic entropy (`H_sem`) with inline `Sparkline` evolution history. Added vertical `BarChart` representing Swarm Persona Confidence ratings. Built detailed `Table` showing active locks (READ/WRITE/IDLE), latencies, and CRDT merges/contention. Upgraded both `ContractNegotiated` lists and `ContractApprovalRequest` modals to render criteria side-by-side with live BERT cosine-similarity scores. Upgraded `src/leader.rs` to compute dynamic similarity and campaign health parameters (velocity, risk, progress, doom_prob), pipelining them directly to the TUI alongside periodic `PersonaTelemetry` lock updates. Verified complete compilation, all 10 tests passing cleanly (`cargo test`), and seamless execution under TUI and headless modes.

### Phase 1: Cognitive Swappable Provider Layer (2026-05-21)

- **Completed Model-Agnostic LLM Provider (`src/llm.rs`)**: Implemented Phase 1, Step 1 of the ASEE competitive roadmap. Designed and developed a zero-SDK pure-HTTP client framework covering OpenAI, Anthropic Claude, xAI Grok, Local Ollama, and a fully stateful, deterministic `MockProvider` for robust offline testing. Features stateful exponential backoff retries and an in-memory `CircuitBreaker`.
- **Registered Module & Dynamic Startup Banner**: Registered `pub mod llm;` in `src/main.rs` and dynamically resolved and printed the active provider state on CLI startup inside the ecosystem diagnostics.
- **Achieved 100% Test Integrity**: Added 4 comprehensive new unit tests validating payload serialization formats and backoff/breaker states. Verified that all 16/16 unit tests compile and execute cleanly with zero regression.

## Related

- [[_GROK.md]] — Operating manual
- [[wiki/decisions/0001 - Project Kickoff]]
- [[wiki/decisions/0002 - Lighter Pragmatic AI-First Approach for Korg]]
- [[wiki/concepts/Korg-Audit-and-Competitive-Roadmap]] — Full audit & roadmap note
- `sources/obsidian-second-brain/` (reference implementation under study)

---

**Next:** Execute Phase 1, Step 2: Wire the newly established `LlmProvider` cognitive abstraction to drive Korg's core multi-persona swarms (Leader, Workers, Evaluator), replacing current prompt simulations with live, swappable LLM reasoning loops.




## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-19, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## See Also

- [[Synthesis — ACP Protocol]]

- [[Synthesis — Contract Negotiation]]

- [[Synthesis — Adversarial Loop]]

- [[Synthesis — Evaluator Persona]]

- [[Synthesis — Transactional Memory]]

- [[Synthesis — Blackboard]]

- [[Synthesis — Semantic Entropy]]
