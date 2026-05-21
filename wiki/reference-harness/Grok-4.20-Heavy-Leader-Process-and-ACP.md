---
date: 2026-05-20
type: reference-architecture
tags: [reference-architecture, acp, leader-broker, grok-heavy, multi-agent, production-system]
harness: korg
domain: acp, orchestration, reference-implementation
status: active
ai-first: true
---

# Reference Architecture: Grok 4.20 Heavy Leader Process and ACP

**This is a high-fidelity extraction of the actual production architecture behind Grok 4.20 Heavy mode (the 16-agent swarm available in the SuperGrok Heavy tier).** It describes the real Leader process, the ACP coordination layer, and how up to 16 independent Grok 4.20 inference runtimes operate in true parallel on xAI’s Colossus cluster.

This document serves as the primary source grounding Korg’s reference-harness layer.

---

## Core AI/ML Serving Infrastructure (Foundation)

The base model is a ~3T-parameter Mixture-of-Experts (MoE) architecture served across the Colossus cluster. The following techniques enable true 16-way parallel independent runtimes:

- **Paged KV Cache** — KV tensors are stored in fixed-size pages (typically 16–32 tokens). Each runtime maintains its own virtual page table. Pages are allocated on-demand and can be evicted independently. This allows 16 agents to have effectively private contexts while sharing physical GPU memory. Page table updates are atomic and logged for provenance.

- **Continuous Batching + Dynamic Scheduling** — The inference engine maintains a global token queue. Every few milliseconds it re-batches tokens from all active agents, prioritizing high-confidence or near-timeout requests. The scheduler uses a cost-aware heuristic balancing load across experts.

- **Prefix Caching / Shared Context** — Identical prefix tokens (shared prompt segments or blackboard state) are stored once in a reference-counted prefix cache. Each agent’s KV cache only stores its unique suffix.

- **Expert Routing Under High Concurrency** — Per-token routing uses a lightweight router. Under 16× load, the system uses grouped routing, expert buffering, and asynchronous expert updates to avoid hot-spot contention.

**Multi-Session / Multi-Runtime Isolation**  
Each of the 16 agents is a fully independent inference runtime with:
- Separate KV page table and decoding loop
- Separate system prompt + RL policy
- Separate tool sandbox and blackboard view (filtered by capabilities)
- Logically partitioned GPU memory via the paged cache

This isolation guarantees that one agent’s context corruption or infinite loop cannot affect others, while still allowing efficient sharing of model weights and prefix cache.

---

### Token-Bucket Throttling & Resource Gating

The gateway uses a dynamic, multi-dimensional token-bucket that tracks *effective compute burn*:

\[
\text{EffectiveBurnRate} = (T_{\text{in}} + T_{\text{out}}) \times (1 + (N_{\text{agents}} - 1) \times \alpha) \times (1 + (\frac{C}{C_{\max}})^2 \times \beta) \times \gamma
\]

Where:
- \( T_{\text{in/out}} \): Base tokens
- \( N_{\text{agents}} \): Active sub-agents (0–16)
- \( \alpha \approx 0.6 \): Concurrency weight
- \( C \): Current context tokens (up to 2M)
- \( \beta \approx 1.6 \): Depth penalty
- \( \gamma \): KV pressure factor (1.0–2.0)

**Pseudocode for Effective Burn Rate & Throttling Decision**

```pseudocode
function calculate_effective_burn(T_in, T_out, N_agents, C, cluster_pressure):
    base = T_in + T_out
    concurrency_mult = 1 + (N_agents - 1) * ALPHA          // ALPHA ≈ 0.6
    depth_penalty = 1 + pow(C / C_MAX, 2) * BETA           // BETA ≈ 1.6
    kv_factor = 1 + cluster_pressure * GAMMA                // GAMMA ≈ 0.8
    return base * concurrency_mult * depth_penalty * kv_factor

function apply_throttling(bucket_remaining, effective_burn):
    if bucket_remaining > WARNING_THRESHOLD:
        return NO_THROTTLE
    elif bucket_remaining > CRITICAL_THRESHOLD:
        delay = (1 - bucket_remaining) * MAX_DELAY_FACTOR
        return {"action": "velocity_throttle", "delay_ms": delay}
    else:
        return {"action": "soft_wall", 
                "measures": ["reduce_max_agents", "partial_synthesis", "cooldown"]}
```

The bucket uses a rolling multi-hour window. When low, velocity throttling or soft walls are applied automatically. This is the primary “kernel-level” guard preventing a single Heavy session from starving the cluster.

---

## Core Architecture & State Machine

The Leader (the main Grok 4.20 session the user interacts with) runs a continuous closed-loop control system with the following phases:

1. **Ingestion & Task Graph Construction**  
   The Leader tokenizes the prompt, runs a lightweight domain classifier (MoE head), and builds a Task DAG. Each node carries `task_id`, `domain_mask`, `payload`, `priority`, `timeout_ms`, and `required_evidence`. Shared context (up to 2M tokens) is partitioned via attention masking so agents see only relevant slices.

2. **Dynamic Routing & Activation**  
   The Leader builds the initial DAG from the root task using a fast domain classifier + complexity/uncertainty estimator. Each node contains: `task_id`, `payload`, `domain_mask`, `timeout_ms`, `permissions`, and `depends_on`.

   **Dynamic Evolution & Pruning**  
   Agents can emit new sub-tasks at any time (`task.create` with parent link) → Leader inserts them live and re-evaluates the graph. Pruning removes completed, low-value, or stalled branches to free resources.

   **Pseudocode for DAG Pruning Logic** (core Leader loop):
   ```pseudocode
   function prune_dag(dag, current_time, metrics):
       for each node in dag.bottom_up():
           if node.status == COMPLETED:
               mark_for_prune(node)          // free KV pages, release worktree
           elif node.status == STALLED and (current_time - node.last_progress > node.timeout):
               cancel_subtree(node)          // send task.cancel to all descendants
               mark_for_prune(node)
           elif node.entropy > ENTROPY_THRESHOLD or node.confidence < CONFIDENCE_FLOOR:
               if should_scale(node):
                   spawn_new_agents(node)    // dynamic routing via RL policy
               else:
                   escalate_to_leader(node)  // conflict.resolve
           elif node.value_estimate < PRUNE_VALUE_THRESHOLD:
               cancel_subtree(node)
               mark_for_prune(node)

       remove_pruned_nodes_and_edges(dag)
       compact_blackboard()                 // reclaim memory
       return dag
   ```

   The `should_scale` check uses the RL policy (cost × expected accuracy gain). Pruning is non-blocking and runs on every major blackboard update.

   The Leader uses a trained RL policy to select the *minimal viable swarm size* (2–3 for simple, 4–8 moderate, up to 16 for extreme depth). Mid-task scaling is triggered by output entropy, conflict rate spikes, DAG expansion, confidence drops, or explicit “deeper” signals. The policy optimizes expected latency × uncertainty while maximizing coverage.

3. **Parallel Execution & Reactive Coordination**  

   ### Native Agent Persona Topology (4-Agent Core + 16-Agent Scaling)

   Grok 4.20 Heavy runs a **native 4-agent topology** for complex reasoning tasks. These four specialized roles operate as **adapter heads** on the same ~3T-parameter MoE backbone (not separate model instances). They share weights, prefix cache, and large portions of the KV cache:

   - **Grok / Captain** (primary Reasoning / Orchestrator) — owns the root task decomposition, high-level planning, and final synthesis.
   - **Harper** (Critique / Research) — deep evidence gathering, citation validation, counter-argument generation, and hallucination pressure-testing.
   - **Benjamin** (Tool-Use / Execution) — concrete tool calling, code execution, file system operations, and external API interaction.
   - **Lucas** (Orchestration / Synthesis) — cross-agent coordination, conflict surfacing, partial result merging, and narrative assembly.

   These four run concurrently with role-specific system prompts and RL-tuned policies. Because they are lightweight heads on a shared backbone, the marginal cost is only **~1.5–2.5×** a single forward pass instead of a naïve 4× (thanks to shared prefix/KV pages and no full re-prefill per agent).

   The **16-agent Heavy variant** is a direct scaling of the same architecture (configurable via `agent_count=16` or `reasoning.effort=high/xhigh`). It replicates and parallelizes the 4-persona pattern across deeper debate trees or broader exploration fronts. The same ACP, blackboard, Arena, and worktree mechanisms apply at any swarm size up to the 16-agent ceiling.

   This topology is explicitly the foundation that **Grok Build** (the terminal-based agentic coding CLI launched mid-May 2026 for SuperGrok Heavy subscribers) exposes to users with plan/review/approve safety gates and ephemeral worktree isolation.

   ---

   Each of the 16 agents is a **fully independent inference runtime** with:
   - Its own KV cache and attention state (paged, using techniques similar to PagedAttention — default 16-token blocks with vLLM-style hybrid eviction and disaggregated prefetch over NVLink/PCIe)
   - Its own autoregressive decoding loop
   - Its own isolated git worktree (in Grok Build) and sandboxed tool execution
   - Role-specific system prompt + RL-optimized policy (drawn from the 4-persona set above, or further specialized for 16-agent depth)

   Communication occurs over ACP. The Leader maintains a central versioned blackboard (KV store) for intermediate results, citations, and confidence scores. Agents communicate via `message.send` (peer or leader-directed). 

   **Blackboard & Context Management Details**
   The blackboard is the central, versioned KV store that all agents read from and write to. It serves as the single source of truth for shared context, intermediate results, citations, and DAG state.

   - **Concurrency Control**: Writes use optimistic concurrency (etags or vector clocks). High-contention keys fall back to short-lived Leader-mediated locks or CRDT-style automatic merge rules. Conflicts are escalated to the Leader as `conflict.resolve` events.

   **Blackboard Merge Pseudocode** (core Leader arbitration logic):

   ```pseudocode
   function merge_blackboard_write(key, new_value, source_agent, trace_id, etag):
       current = blackboard.get(key)  // with current etag and vector_clock
       
       if current.etag != etag:  // optimistic concurrency failure
           if is_crtd_compatible(key):  // e.g. sets, counters, lists
               merged = crdt_merge(current.value, new_value)
               blackboard.put(key, merged, new_etag=generate_etag(), vector_clock=advance(current.vector_clock))
               log_provenance(key, source_agent, trace_id, "auto_merged")
               return SUCCESS
           else:
               // Conflict requires human/Leader judgment
               conflict_event = {
                   "type": "conflict.resolve",
                   "key": key,
                   "current": current.value,
                   "proposed": new_value,
                   "source_agent": source_agent,
                   "trace_id": trace_id
               }
               leader_queue(conflict_event)
               return PENDING_ARBITRATION
       
       // No conflict - fast path
       blackboard.put(key, new_value, new_etag=generate_etag(), vector_clock=advance(current.vector_clock))
       log_provenance(key, source_agent, trace_id, "direct_write")
       return SUCCESS

   function log_provenance(key, agent, trace, op_type):
       audit_log.append({
           "key": key,
           "agent_id": agent,
           "trace_id": trace,
           "operation": op_type,
           "timestamp": now(),
           "confidence": agent.confidence_score
       })
   ```

   This merge routine runs on every blackboard write attempt and ensures safe concurrency while preserving full provenance for later Arena Mode scoring or audit. High-contention keys fall back to short-lived Leader-mediated locks or CRDT-style automatic merge rules. Conflicts are escalated to the Leader as `conflict.resolve` events.

   - **Delta Patching**: Updates are sent as fine-grained diffs (token ranges, embedding deltas, or structured key-value patches). Application is atomic per agent. Failed patches trigger automatic rollback to the agent’s last consistent checkpoint with re-sync from the blackboard.
   - **Provenance Attachment**: Every write carries agent_id, trace_id, timestamp, confidence, and source fragment reference. This enables full per-claim audit trails and supports Arena Mode scoring.
   - **KV Cache Strategy**: Each runtime has its own paged KV cache (vLLM-style PagedAttention with 16-token blocks + hybrid eviction). Common prefix tokens are reference-counted and shared across agents. Under memory pressure the engine evicts pages using a recency × importance heuristic and can recompute or swap to host memory (tiered to fast NVMe with predictive hydration).

   **External validation of KV reuse** — The xAI API exposes discounted “Cached Input” pricing (commonly ~$0.20/M tokens vs $1.25–$2/M for fresh input). This is direct evidence that the production stack aggressively reuses prefix/KV state across agents and across turns, exactly as described in the architecture above.

   Concurrent blackboard writes are handled with optimistic concurrency + vector clocks; conflicts are mediated by the Leader.

4. **Conflict Arbitration & Verification**  
   When outputs diverge, the Leader triggers an internal “Arena” round. Each conflicting agent self-critiques and emits a structured score vector (accuracy, coherence, novelty, risk, efficiency, etc.) plus rationale. Scores are numerical (typically 0–1 per dimension) with confidence weights.

   **Self-Scoring**  
   Each agent receives the full set of competing outputs + shared blackboard context and emits:
   ```json
   {
     "scores": {
       "accuracy": 0.92,
       "coherence": 0.88,
       "novelty": 0.75,
       "risk": 0.15,
       "efficiency": 0.91
     },
     "confidence": 0.87,
     "rationale": "Strong evidence from X sources; minor edge-case uncovered"
   }
   ```

   **Aggregation & Voting**  
   The Leader computes a weighted composite (weights = domain expertise + historical reliability). For pure voting, agents rank outputs. Ties are broken by Leader meta-reasoning (higher confidence first, then alignment with user intent, then evidence strength) or by forcing a re-run with additional context.

   **Resolution Flow**  
   Conflict detected → Arena Mode triggered (automatically or via `conflict.resolve`).  
   Agents self-score in parallel.  
   Leader aggregates → selects winner (or hybrid).  
   Winning output committed to blackboard; losers archived with full provenance.  
   All scores and rationales logged for audit.

   This directly powers the Merge-Arbitration Engine and CONTESTED artifact handling in Korg. Hallucination filters and citation validation also run at this layer.

5. **Synthesis & Output**  
   The Leader merges DAG results in a final synthesis pass (sometimes delegated to a dedicated orchestrator sub-agent). Output is streamed back with optional provenance traces. Post-synthesis RL reward signals (accuracy + coherence + user satisfaction) feed online fine-tuning.

The loop is governed by a token-budget scheduler and a convergence heuristic (entropy of agent outputs dropping below a threshold).

**Epistemic State Machine & Blackboard Architecture**

The blackboard operates as a strict finite-state machine for every claim/artifact. Transitions are deterministic and trigger specific Leader actions:

- **OBSERVED** → **INFERRED**: Triggered by a sub-agent’s first reasoning pass or tool output. No verification yet.
- **INFERRED** → **VERIFIED**: Requires successful deterministic validation (lint/compile/test/proof) inside the agent’s isolated environment. The agent must attach a verifiable artifact (test case, dependency graph slice, etc.).
- **INFERRED / VERIFIED** → **CONTESTED**: Any peer agent raises a structural, logical, or semantic conflict via `conflict.resolve`.
- **CONTESTED** → **VERIFIED** (or rejected): Arena Mode forces proof generation. Leader synthesizes the winning proof and commits the state.

**Pseudocode for the core state transition handler** (runs on every blackboard write):

```pseudocode
function transition_blackboard_state(key, new_state, proof_artifact, source_agent):
    current = blackboard.get_state(key)
    
    if current == OBSERVED and new_state == INFERRED:
        blackboard.set_state(key, INFERRED, source_agent, proof_artifact)
    elif current == INFERRED and new_state == VERIFIED:
        if validate_proof(proof_artifact):  // lint, test, compile, etc.
            blackboard.set_state(key, VERIFIED, source_agent, proof_artifact)
        else:
            raise_error("VERIFICATION_FAILED")
    elif new_state == CONTESTED:
        trigger_arena_mode(key, source_agent, proof_artifact)  // spawns proof contest
    elif current == CONTESTED and new_state == VERIFIED:
        if leader_arbitrate(proof_artifact):  // Arena winner
            blackboard.set_state(key, VERIFIED, winner_agent, proof_artifact)
        else:
            blackboard.set_state(key, REJECTED, source_agent, proof_artifact)
    
    log_provenance(key, current, new_state, source_agent)
    return new_state
```

Every transition is logged with full provenance and can trigger Leader actions such as Arena Mode or capability revocation.

---

### Task DAG Construction & Dynamic Routing

The Leader builds the initial DAG from the root task using a fast domain classifier + complexity/uncertainty estimator. Each node contains: `task_id`, `payload`, `domain_mask`, `timeout_ms`, `permissions`, and `depends_on`.

**Dynamic Evolution & Pruning**

Agents can emit new sub-tasks at any time (`task.create` with parent link) → Leader inserts them live and re-evaluates the graph. Pruning removes completed, low-value, or stalled branches to free resources.

**Pseudocode for DAG Pruning Logic** (core Leader loop):

```pseudocode
function prune_dag(dag, current_time, metrics):
    for each node in dag.bottom_up():
        if node.status == COMPLETED:
            mark_for_prune(node)          // free KV pages, release worktree
        elif node.status == STALLED and (current_time - node.last_progress > node.timeout):
            cancel_subtree(node)          // send task.cancel to all descendants
            mark_for_prune(node)
        elif node.entropy > ENTROPY_THRESHOLD or node.confidence < CONFIDENCE_FLOOR:
            if should_scale(node):
                spawn_new_agents(node)    // dynamic routing via RL policy
            else:
                escalate_to_leader(node)  // conflict.resolve
        elif node.value_estimate < PRUNE_VALUE_THRESHOLD:
            cancel_subtree(node)
            mark_for_prune(node)

    remove_pruned_nodes_and_edges(dag)
    compact_blackboard()                 // reclaim memory
    return dag
```

The `should_scale` check uses the RL policy (cost × expected accuracy gain). Pruning is non-blocking and runs on every major blackboard update.

---

### Capability & Permission Model (Deep Dive)

Capabilities are granted at `task.create` as a scoped set attached to the task:

Example Scopes:
- `fs:read:/src/**`
- `fs:write:worktree-only` (restricted to the agent’s private git worktree)
- `exec:safe-commands` (whitelisted shell ops)
- `network:web-search-only`
- `context:read:blackboard:subset`

The Leader is the sole policy enforcement point. Every tool invocation is mediated through ACP `tool.invoke`; the Leader checks the capability before proxying. Mid-task revocation is possible via `capability.revoke` (on policy violation or user intervention). Sub-agents never receive raw OS-level handles — all access is sandboxed and logged.

---

### Grok Build CLI Internals

Grok Build is the official terminal-based agentic coding tool that exposes the full Heavy multi-agent swarm with strong emphasis on local filesystem isolation and plan/review/approve safety gates.

**Key Mechanics** (detailed in dedicated note `Grok-Build-CLI-Internals.md`):
- **Worktree Management**: Each sub-agent gets its own isolated git worktree (`git worktree add`). Changes stay isolated until the Leader merges approved diffs.
- **Tool Proxying**: All tool calls are proxied through the Leader via ACP `tool.invoke`. The Leader enforces capability scopes and audits every action.
- **Plan / Review / Approve Loops**: Explicit staged workflow (plan → parallel execution in worktrees → Arena ranking of diffs → human/policy-gated merge).
- The CLI acts as a thin ACP client: translates user input into `task.create` and approval events, while heavy orchestration (blackboard, Arena, etc.) happens on the backend.

See [[wiki/reference-harness/Grok-Build-CLI-Internals.md]] for the full extraction.

---

## ACP as the Coordination Layer

ACP (Agent Control / Client Protocol) is the standardized JSON-RPC 2.0-based protocol that serves as the “LSP for agents.”

**Transport options:**
- Local: stdio / Unix sockets (sub-process model in Grok Build)
- Distributed: HTTP/WS with TLS + mutual auth

**Core message envelope** uses standard JSON-RPC with a `method` field and structured `params`.

**Key methods used in production Heavy mode:**
- `capabilities.get`
- `task.create` / `task.update` / `task.cancel`
- `message.send` (peer-to-peer or leader-directed)
- `result.stream` (SSE-style partial results with incremental provenance)
- `conflict.resolve`
- `permissions.request`
- `audit.log`

**Error handling:** Standard JSON-RPC + domain extensions (e.g., `DOOM_LOOP_DETECTED`, `CONTEXT_EVICTED`, `TASK_TIMEOUT`). Transient errors use exponential backoff with jitter; permanent errors escalate to the Leader.

Every message carries a provenance chain (signed tokens) and `trace_id` for end-to-end attribution.

---

## How the Leader + 16 Agents Bind Together

- The Leader hosts an internal ACP server endpoint.
- Each sub-agent is a full, independent Grok 4.20 replica with its own runtime, KV cache, and filesystem sandbox (git worktree in coding workflows).
- Tool calls are proxied through the Leader (or sandboxed executors) for policy enforcement.
- Shared context is synchronized via compact delta patches (`context.patch`) rather than full re-sends. Patches are atomic and versioned.
- In Grok Build headless/daemon mode, the CLI starts the Leader, which then orchestrates up to 8–16 parallel sub-agents across isolated worktrees.

**Resource model:** Model weights (MoE) are shared. Each agent maintains independent KV cache pages. Continuous batching + dynamic scheduling treats the agents as concurrent high-priority inference jobs. SuperGrok Heavy users receive priority or dedicated GPU partitions for the session.

---

## Security, Isolation & Permissions

- **Capability-based model**: Permissions (filesystem scopes, network access, tool access, context visibility) are granted at `task.create` time and enforced by the Leader (the policy enforcement point).
- **Isolation layers**: Separate process/runtime sandboxes, per-agent git worktrees, mediated tool execution, and message-only inter-agent communication.
- **Information leakage prevention**: Default-deny scoping + full provenance tracking. The Leader can revoke capabilities mid-task.

**Capability & Permission Model (Deep Dive)**
Capabilities are granted at `task.create` as a scoped set (example):
- `fs:read:/src/**`
- `fs:write:worktree-only` (restricted to the agent’s private git worktree)
- `exec:safe-commands` (whitelisted shell ops)
- `network:web-search-only`
- `context:read:blackboard:subset`

The Leader is the sole policy enforcement point. Every tool invocation is mediated through `tool.invoke` ACP messages; the Leader checks the capability before proxying. Mid-task revocation is possible via `capability.revoke` (on policy violation or user intervention). Sub-agents never receive raw OS-level handles — all access is sandboxed and logged.

---

## Failure, Recovery & Doom-Loop Handling

- Failures are detected via heartbeats and ACP error events.
- **Productive death** (agent delivered useful partial work before termination) is distinguished from **doom-loop death** (repetitive low-progress cycles) using progress metrics (token velocity, novelty, conflict rate).
- Recovery options: re-spawn with blackboard replay, re-route sub-task, partial synthesis from already-committed results, or human escalation.
- Critical paths use checkpointing. The system retains useful partial work with clear “partial/incomplete” provenance marking.

---

## Observability & Monitoring Signals

The Leader exposes rich telemetry:

- Per-agent: progress %, confidence trajectory, token velocity, conflict rate, KV page usage, GPU utilization.
- System: DAG growth, swarm entropy, blackboard contention, latency breakdown.
- STALLED / routing liveness events surface as high-priority notifications that can trigger auto-scaling or re-routing.

Provenance is token/claim-granular. Operators watch for divergence metrics, latency spikes, and “healthy swarm” patterns (balanced contribution, rapid convergence).

---

## Performance & Cost

- Wall-clock latency scales sub-linearly (sweet spot typically 4–8 agents; diminishing returns beyond ~12 due to synthesis overhead).
- Typical cost multiplier vs single-pass: **~1.5–2.5×** for the native 4-agent topology; up to ~5–8× for full 16-agent mode (still far below naïve scaling because of shared MoE weights, prefix caching, and paged KV reuse across the swarm).
- The RL policy aggressively optimizes the accuracy/latency/cost trade-off in real time.

---

## Headless & Programmatic Usage

The swarm is driven via the same ACP interface (or thin REST/WS wrappers):

- Authentication via subscription-tied session tokens or API keys.
- Headless mode supports streaming JSON events, named sessions, and `--session <id>` resume.
- Full DAG control, sub-agent management, and audit export are available.
- CI/CD-friendly via stdio or WebSocket daemon interface.

**Grok Build CLI Internals (see dedicated note)**
Grok Build is the official terminal client that exposes the full Heavy swarm with strong emphasis on local filesystem isolation and plan/review/approve safety gates. Key mechanics include per-agent git worktree isolation, tool proxying through the Leader (all calls via ACP `tool.invoke`), and explicit staged loops (plan → parallel execution in worktrees → Arena ranking of diffs → human or policy-gated merge). The CLI acts as a thin ACP client that translates user input into `task.create` and approval events while the heavy orchestration happens on the backend. See [[wiki/reference-harness/Grok-Build-CLI-Internals.md]] for full details.

---

## Concrete Mappings to Korg

| Grok 4.20 Heavy Element                        | Korg Primitive / Note                                                                 |
|-----------------------------------------------|---------------------------------------------------------------------------------------|
| Leader as central policy brain + RL router    | Leader role in Leader-Broker-ACP Model                                               |
| Broker-like execution supervisors             | Broker responsibilities in isolation-routing + transactional-memory                  |
| Paged KV Cache, Continuous Batching, Prefix Caching, Multi-Runtime Isolation | Core serving infrastructure enabling true parallel independent runtimes             |
| Token-Bucket Throttling & Resource Gating     | Kernel-level gating model for fair multi-tenant resource allocation                  |
| Independent runtimes + worktree isolation     | Ephemeral Worktree Isolation + per-agent git worktrees                               |
| ACP (`task.create`, `message.send`, `conflict.resolve`, `result.stream`, `tool.invoke`) | ACP message surface in ACP-Binding-Design + Message-Schema                           |
| Central blackboard + delta patching + versioning + CRDT merge | Transactional Memory + merge/rebase logic                                            |
| Epistemic State Machine (OBSERVED → INFERRED → VERIFIED → CONTESTED) | Epistemic State Machine in state-primitives.md                                       |
| Doom-loop detection + productive death distinction | Non-Interactive Doom-Loop Detection + terminal `.ktrans` + recovery strategies     |
| Arena + weighted arbitration                  | Merge-Arbitration Engine + authority vectors + human `semantic-decision` gate       |
| Capability-based permissions + Leader enforcement | Security model for reference harnesses                                               |
| Rich per-agent + system telemetry             | Live campaign monitoring signals (velocity, STALLED, contest pressure, entropy)     |
| Headless/daemon swarm usage                   | Reference harness headless patterns and ACP programmatic usage                       |

---

## Gaps / Open Questions for Our Reference Design

While this document now provides excellent production depth, several areas remain partially opaque or implementation-specific:

- Exact internal RL reward weights and the full scheduling heuristics for swarm sizing.
- Precise conflict resolution algorithm inside the blackboard (how CRDTs, vector clocks, and Leader arbitration interact in practice).
- Full standardized schema for the observability/telemetry stream and audit export format.
- Complete authentication and mutual trust model for distributed ACP (beyond high-level TLS + capability tokens).
- How “xHigh” reasoning mode (full 16 agents) is triggered and cost-accounted at the subscription layer.
- Exact constants and tuning parameters for the token-bucket throttling formula in different cluster regions.

These represent natural extension points for future Korg reference work (e.g., dedicated notes on Token-Bucket Gating and Serving Infrastructure / KV Cache Lifecycle).

---

## Related

- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — Cross-lab validation of long-running harness patterns (Generator/Evaluator adversarial loops, persistent artifacts, Planner + Generator + Evaluator roles) that map strongly to the Leader-Broker-Worker model and Arena Mode described here.

- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine, doom-loop detection, and Merge-Arbitration Engine.
- [[wiki/mechanisms/isolation-routing.md]] — Worktree isolation, STALLED handling, and back-pressure.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` handoff, rebasement, and rollback isolation.
- [[wiki/reference-harness/ACP-Binding-Design.md]] — Minimal ACP protocol surface (faithful abstraction of the real Grok Heavy ACP).
- [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] — Coordination model directly derived from the Grok 4.20 Heavy Leader process.
- [[wiki/reference-harness/ACP-Message-Schema.md]] — Concrete message schemas and wire formats derived from this architecture.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Practical pseudocode harness that speaks the ACP and demonstrates the full Grok Build-style plan → Arena → review/approve loop in working form.
- [[wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md]] — Focused deep-dive on the dynamic token-bucket model and resource gating.
- [[wiki/reference-harness/Serving-Infrastructure-and-KV-Cache-Lifecycle.md]] — Focused deep-dive on the serving stack and KV cache lifecycle.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — Production-grade workflow patterns and personas built on top of this system.
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — How to implement harnesses that speak this ACP and follow the same Leader-Broker discipline.

This note establishes the production reality against which all Korg reference-harness work is measured.