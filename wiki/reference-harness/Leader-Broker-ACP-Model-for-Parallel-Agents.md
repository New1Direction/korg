---
date: 2026-05-20
type: design
tags: [design, acp, leader-broker, reference-harness, orchestration, parallel-execution]
harness: korg
domain: acp, orchestration, state-management
status: emerging
ai-first: true
---

# Leader-Broker ACP Model for Parallel Agents

This note describes the coordination model that sits on top of the minimal ACP defined in `ACP-Binding-Design.md`. It shows how a **Leader** (strategic planner) and one or more **Brokers** (execution supervisors) can safely drive many concurrent, isolated workers while preserving the guarantees of the three core Korg contracts.

The model is the natural realization of the "Leader-Broker-ACP-Model-for-Parallel-Agents" pattern that was referenced throughout the mechanism and human layers. It turns the low-level ACP messages into a scalable, failure-resilient architecture for headless offensive work, swarms, and long-running campaigns.

**Traceability**: This note is a faithful abstraction of the Grok 4.20 Heavy architecture described in [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]]. The Leader-Broker separation, RL-driven dynamic swarm sizing, mid-task scaling triggers, work dispatch, conflict arbitration (Arena), blackboard concurrency, productive-death recovery, and capability-based permission model are all directly derived from the real Grok Heavy Leader process and ACP coordination layer.

---

## Core Roles and Responsibilities

| Role    | Primary Concerns                                      | Owns (from the triad)                          | Communicates via ACP |
|---------|-------------------------------------------------------|------------------------------------------------|----------------------|
| **Leader** | Strategic planning, capability matching, campaign scope, high-level routing decisions | Overall epistemic goals, priority of attack surfaces | `RouteWork`, receives `AckRoute` / `Stalled` / `ContestedNotification` aggregates |
| **Broker** | Worker allocation, worktree isolation, epoch enforcement, back-pressure, merge queue, termination | Physical isolation, routing liveness, `.ktrans` handoff, doom-loop detection | All worker-facing messages (`WorkerHello`, `Heartbeat`, `SubmitTransaction`, `TerminationReport`, etc.) |
| **Worker** | Actual tool execution inside an isolated worktree    | Producing correct, provenanced `.ktrans` payloads | `WorkerHello`, `Heartbeat`, `SubmitTransaction`, `TerminationReport` |
| **Client** (optional) | Human or higher-level orchestrator observing the system | Strategic overrides, contested-fact review     | Observes `ContestedNotification`, `MergeOutcome`, `StatePromotion` streams |

The Leader never touches workers or worktrees directly. The Broker never makes strategic decisions. This separation is what allows the model to scale from a single TUI process to a large distributed swarm.

---

## Work Dispatch and Routing at Scale

When the Leader decides that a particular attack surface or capability should be exercised, it emits a `RouteWork` message to one or more Brokers.

A `RouteWork` payload contains:
- `routing_id`
- Required capabilities
- `base_snapshot` hash (the verified state the worker is allowed to start from)
- Epoch deadline
- Optional priority and blast-radius hints

The Broker that accepts the work:
- Allocates (or spawns) a Worker inside a fresh, cryptographically isolated worktree
- Mounts only the minimal verified snapshot
- Returns `AckRoute` (or `Stalled` if it cannot honor the epoch)

If a Broker fails to acknowledge within the epoch window, it must emit a `Stalled` notification. This notification is visible to the Leader and to any Clients, and it triggers the Broker to mark the routing target appropriately so dependent claims can be contested.

At scale, a Leader can fan out the same `RouteWork` (or semantically similar work) to multiple Brokers for load distribution or redundancy. Each Broker operates independently on its own pool of workers.

---

## Transaction Handoff and Rebasement Across Workers

Workers only ever communicate state changes by emitting `SubmitTransaction` messages containing `.ktrans` payloads.

The Broker is responsible for:
- Structural and provenance validation of every incoming `.ktrans`
- Placing valid transactions into a per-Broker (or global) merge queue
- Performing the headless three-way rebase when a transaction arrives with a stale `base_snapshot`
- Emitting `ContestedNotification` or `MergeOutcome` messages when arbitration occurs

Because multiple Brokers may be active simultaneously, the model assumes either:
- A single shared merge service that all Brokers feed into, or
- Eventually consistent merge with a clear authority-vector tie-breaker when conflicts cross Broker boundaries.

In either case, the `tx_id` (UUIDv7) provides a globally comparable ordering key for rebasement decisions.

---

## Doom-Loop Detection and Termination Coordination

Doom-loop detection is a Broker responsibility (see `state-primitives.md` and `isolation-routing.md`).

The Broker monitors the `Heartbeat` stream from each worker for the combined signal of high token velocity + near-zero AST/semantic delta.

When the threshold is crossed (classically three consecutive qualifying heartbeats), the Broker:
1. Sends `RequestTerminate` with reason `doom_loop`
2. Waits briefly for a `TerminationReport` containing the `tx_id` of the final diagnostic `.ktrans`
3. If no report arrives, the Broker itself may synthesize a minimal terminal transaction describing the kill
4. Emits the appropriate `ContestedNotification` for any in-flight claims from that worker

This flow ensures that even aggressively terminated workers still contribute their last coherent findings to the vault.

The Leader is notified of the termination via aggregated `ContestedNotification` or `TerminationReport` streams, but does not directly kill workers. This preserves the separation of concerns.

---

## Support for Different Harness Styles

The same Leader-Broker model can be deployed in very different configurations:

**Single-Worker / TUI Style (Pattern A from the harness note)**
- Leader + Broker often collapsed into the same process
- One (or a small number of) Worker(s) at a time
- `ContestedNotification` messages are rendered directly to the human operator
- Epoch windows can be generous or even disabled for interactive use

**True Multi-Worker Swarm (Pattern B)**
- Leader is a relatively lightweight strategic planner (possibly running on a different machine)
- Multiple independent Brokers, each managing their own worker pools and worktrees
- Brokers may be co-located with workers for locality or distributed for scale and failure domains
- Strong reliance on `Stalled` signaling, back-pressure, and aggregated observability streams

**Pipeline / Background Daemon Style**
- Long-lived workers are common
- Heartbeat messages carry richer progress information
- Termination is rarer and usually graceful
- The model still works; the Broker simply rarely exercises the doom-loop path

All styles still honor the five core harness responsibilities and emit the same ACP message types.

---

## Extension Points Specific to Parallel Execution

- **Capability vectors** — Richer descriptions in `RouteWork` so the Leader can match work to specialized Brokers or worker types.
- **Blast-radius hints** — Optional metadata on `RouteWork` that helps Brokers and Clients prioritize which contested facts to surface first.
- **Broker federation** — Protocols for Brokers to hand off work or share merge state when a campaign spans multiple failure domains.
- **Observability aggregation** — Standardized ways for Brokers to publish velocity, STALLED, and contest-rate metrics so a central dashboard can watch an entire swarm.
- **Priority and preemption** — Ability for the Leader to instruct a Broker to pause or deprioritize lower-value work when higher-value opportunities appear.

---

## Open Questions

- Exact semantics of cross-Broker merge consistency (strong vs. eventual, single vs. sharded merge service).
- How to handle Broker failure or restart without losing in-flight merge decisions.
- Standardized schema for the aggregated observability streams that Clients (including human dashboards) will consume.
- Authentication and authorization model between Leader, Brokers, and Workers at scale.
- How to express "campaign-level" goals (e.g., "map the entire auth surface before deep fuzzing begins") in a way that the Leader can translate into `RouteWork` decisions.

---

## Related

- [[wiki/reference-harness/ACP-Binding-Design.md]] — The minimal protocol messages that this coordination model uses.
- [[wiki/reference-harness/ACP-Message-Schema.md]] — Exact schemas, wire formats, and encoding rules for all messages.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — A production implementation of the same Leader-Broker coordination model.
- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground truth production architecture (RL scheduling policy, blackboard + delta patching, Arena Mode, capability model, productive-death handling, and headless swarm usage) from which this coordination model is derived.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Practical pseudocode that demonstrates the Leader-Broker model in working form.
- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine, doom-loop detection, and Merge-Arbitration Engine that the model must preserve.
- [[wiki/mechanisms/isolation-routing.md]] — Routing, worktree isolation, STALLED handling, and back-pressure that the Leader-Broker relationship enforces.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` handoff and rebasement flows that cross multiple workers and Brokers.
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — The two common patterns (single-worker TUI vs. true swarm) that this model supports.
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — The signals (velocity, STALLED, contest pressure) that a Leader-Broker deployment must make observable.
- [[wiki/patterns/Cross-Harness-Pattern-Extraction.md]] — The long-term vision that this model helps realize across many different harnesses.

This note, together with `ACP-Binding-Design.md`, gives a complete high-level picture of how parallel, failure-resilient, contract-faithful execution can be achieved with the Korg kernel. Subsequent work can now move into detailed message schemas, reference implementations, or metrics if desired.