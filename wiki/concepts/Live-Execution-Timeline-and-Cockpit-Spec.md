---
title: "Live Execution Timeline and Cockpit Spec"
date: 2026-05-21
type: concept
tags: [concept, cockpit, ux, spec, live-execution-timeline, policy-engine, korg]
status: active
ai-first: true
confidence: high
---

## For future Grok

This specification documents Korg's definitive product philosophy: an **Autonomous Software Engineering Environment (ASEE)**. It defines the core visual and behavioral mechanics of the **Live Execution Timeline (Cognitive Git)**, specifies the exact interaction design for the **Replay Scrubber & Time-Travel Forking**, and establishes the declarative architecture for the **Enterprise Policy Engine Primitives** that govern autonomous execution scopes.

---

# Korg Cockpit: Live Execution Timeline & Policy Engine Spec

Korg is not an AI editor extension or an autocomplete assistant. Korg is an **Autonomous Software Engineering Environment (ASEE)** — a unified runtime and editing surface where human intent and autonomous multi-agent execution coexist on a single, continuous timeline.

The IDE is not a text editor; it is the **Orchestration Cockpit** for the Korg runtime.

---

## 1. The Core UX Primitive: The Live Execution Timeline (Cognitive Git)

In Korg, agent cognition is treated exactly like source code: a version-controlled, branching timeline of cryptographically signed state mutations. Every step taken by a worker (planning, tool invocation, evaluation, merging) is a transaction serialized as a `.ktrans` entry.

```
       [Human Prompt]
             │
      (Materialize Graph)
             │
      ┌──────┴──────┐
      ▼             ▼
  [Harper]      [Benjamin]  <-- (Isolated Worktrees)
   (ktrans)      (ktrans)
      │             │
      └──────┬──────┘
             ▼
      [Evaluator Crit]
             │
      [Semantic Merge] ──► (Timeline Commit: tx_v1)
             │
      ┌──────┴──────┐
      ▼             ▼
   [Lucas]       [Sophia]
   (ktrans)      (ktrans)
      │             │
      └──────┬──────┘
             ▼
      [Semantic Merge] ──► (Timeline Commit: tx_v2)
             │
      [Operator Scrubber] ──► (Fork Timeline at tx_v1)
             │
             └─► [Branch Swarm] ──► (Timeline Commit: tx_v2_fork)
```

### Visual Metaphor: The Cognitive Stream
The cockpit renders the **Live Execution Timeline** as a vertical, high-contrast tree diagram.
*   **Nodes**: Represent discrete `.ktrans` commits (JCS canonicalized, Ed25519 signed).
    *   *System Nodes (Blue)*: Graph materialization, contract negotiation, and semantic merges.
    *   *Worker Nodes (Yellow/Green)*: Concurrent worker execution steps, active tool invokers, and intermediate code edits.
    *   *Evaluator Nodes (Red/Green)*: Pass/Fail rubric verdicts and semantic entropy gauges.
    *   *Human Nodes (Purple)*: Approvals, manual interventions, and policy overrides.
*   **Connecting Edges**: Color-coded streams indicating information flow, token consumption, and state dependency.
*   **Live Sparklines**: Integrated directly into nodes, showcasing worker confidence, memory latencies, and tool execution success rates in real-time Crossterm/Ratatui cells.

---

## 2. Interaction Design: The Replay Scrubber & Time-Travel Forking

Because the Korg runtime decouples state persistence (held in the central CRDT Blackboard) from worker subprocess execution, the timeline is fully deterministic. This enables the ultimate cockpit interaction: **Time-Travel Forking**.

### The Replay Scrubber Feel
The Operator can interactively drag a visual timeline slider (the **Replay Scrubber**) in the cockpit:
1.  **Freeze Stream**: As the scrubber moves back, the active campaign pauses.
2.  **State Rehydration**: The cockpit sends a `RouteWork::Replay` ACP frame. The Leader reads the Merkle-verified `.ktrans` journal up to the selected transaction ID (`tx_id`), purging all subsequent mutations from the Blackboard.
3.  **Visual Reversion**: The Monaco editor pane instantly rolls back its buffer to match the exact code state at that instant, while the Telemetry Table displays the historical lock map and latency variables.
4.  **Verification Ticker**: The operator sees a micro-diff showing exactly what files, directories, and memory variables are reverted.

### Time-Travel Forking (The Killer DX Flow)
When the operator scrubs back to a historical node (e.g., `tx_18` before a failed semantic merge at `tx_19`), they can hit **`F`** to **Fork the Swarm**:
*   The cockpit prompts the operator for a **Steering Directive** (e.g., *"Focus Lucas's attention on thread safety, increase the Evaluator's Churn rubric weight to 0.85, and restrict ShellExec permissions"*).
*   The Leader branch-clones the workspace (`git worktree add /tmp/korg/forks/<fork-id> HEAD`), re-initializes the Blackboard with the chosen directive, and launches a fresh, isolated branch swarm.
*   The timeline visually splits, creating a new branch tree (`tx_18 -> tx_19_fork -> tx_20_fork`) flowing parallel to the old defunct path.

---

## 3. Enterprise Policy Engine Primitives (Policy Gating)

As autonomy scales, safety and budget control must be absolute. Korg implements a declarative, zero-trust **Policy Engine** running natively inside the Leader’s coordination loop.

### Declarative Contract: `korg.toml` / `POLICY.md`
System and project-level boundaries are defined in a structured policy ledger:

```toml
[policy.execution]
max_concurrent_workers = 8
max_effective_burn_rate = 0.75 # Token-bucket velocity cap
human_approval_gate_threshold = 0.85 # Automatic merge below this, human gate above

[policy.sandboxing]
network_outbound = "block" # Options: allow, block, ask
write_path_whitelist = ["src/**/*.rs", "tests/**/*.rs", "Cargo.toml"]
read_path_blacklist = ["**/.env", "**/id_rsa", "~/.ssh/**"]

[policy.tools]
shell_execution_allowlist = ["cargo test", "cargo check", "git diff"]
max_tool_execution_timeout_ms = 15000
```

### The Policy Evaluation Primitive
Whenever a worker attempts a tool execution or plans a code patch, the request is framed as a `RouteWork` envelope and checked against the active policy:

1.  **Evaluator Persona Interception**: The Evaluator checks the action against the declarative bounds.
2.  **Authority Rubric Gating**:
    *   *If allowed*: The tool executes, and the telemetry pulse registers resource metrics.
    *   *If whitelisted with restrictions (e.g., custom shell command)*: The Evaluator calculates the semantic distance from allowed commands.
    *   *If blocked*: The Evaluator instantly emits an `EvaluationVerdict::Fail` with a `CONTESTED` state.
3.  **The Cockpit Interrupt**: The ASEE suspends execution, highlights the timeline node in flashing high-contrast magenta, and pops up the **Operator Override Dialog** detailing the policy violation, allowing the operator to **Approve Once**, **Deny**, or **Scrub & Fork**.

---

## 4. Cockpit Layout: The Visual Surface Map

To maintain a thin, fast, and obsessively observable cockpit, the UI is partitioned into 6 high-contrast grids:

```
┌──────────────────────────────────────┬──────────────────────────────────────┐
│                                      │                                      │
│                                      │    Swarm Health & Telemetry          │
│                                      │    • Entropy (H_sem): [ 0.42 ] [====]│
│      Monaco-Based Editor             │    • Burn Rate:       [ 0.65 ] [=== ]│
│      (Dynamic Code & Diff Buffer)    │    • Active Locks:    [ READ  ]      │
│                                      │    • Latency (p95):   [ 124ms ]      │
│                                      ├──────────────────────────────────────┤
│                                      │                                      │
│                                      │    Live Execution Timeline (DAG)     │
│                                      │    o- [tx_01] Leader Init (Approved) │
│                                      │    o- [tx_02] Swarm Dispatched       │
│                                      │    ├───o [tx_03] Harper Edit (ktrans)│
│                                      │    └───o [tx_04] Benjamin (crashed)  │
│                                      │    o- [tx_05] RECOVERY: Re-spawned  │
│                                      │    o- [tx_06] Evaluator (PASS: 0.94) │
│                                      │    *===► [ACTIVE PLAYHEAD]           │
├──────────────────────────────────────┼──────────────────────────────────────┤
│                                      │                                      │
│      Interactive Terminal Pane       │    Git Diff & Provenance Viewer      │
│      (Piped subprocess stdio)        │    • Commit Root: [ 0x8a92f0c... ]   │
│                                      │    • Signed By:   [ Captain/Ed25519 ]│
│                                      │    • Impact:      [ +124 / -18 lines]│
└──────────────────────────────────────┴──────────────────────────────────────┘
[Scrubber Playhead Slider: ──────────────────────────────●─────────────────────────]
```

---

## Related Notes & Links

*   **Operating Manual**: [[_GROK.md]]
*   **Strategic Audit & Roadmap**: [[wiki/concepts/Korg-Audit-and-Competitive-Roadmap|Korg-Audit-and-Competitive-Roadmap]]
*   **State Primitives**: [[wiki/mechanisms/state-primitives|state-primitives]]
*   **Isolation & Routing**: [[wiki/mechanisms/isolation-routing|isolation-routing]]
*   **Transactional Memory**: [[wiki/mechanisms/transactional-memory|transactional-memory]]
*   **Evaluation & Guardrails**: [[wiki/patterns/Evaluation-Guardrail-Layer|Evaluation-Guardrail-Layer]]
*   **Daily Log**: [[wiki/daily/2026-05-21|2026-05-21 Daily Log]]
*   **Audit Journal**: [[log.md]]
