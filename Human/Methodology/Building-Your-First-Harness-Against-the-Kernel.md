---
date: 2026-05-20
title: Building Your First Harness Against the Kernel
type: methodology
tags: [methodology, harness, implementation, contracts]
---

# Building Your First Harness Against the Kernel

A **Korg harness** is the thin, opinionated layer that turns the three core contracts into something a human (or another agent) can actually drive.

It does not replace your tools, agents, or orchestrators. It is the adapter that makes them speak the kernel’s language:

- Respect worktree isolation
- Emit well-formed `.ktrans` transactions (micro + terminal)
- Handle routing liveness (`STALLED` acknowledgments)
- Preserve provenance and base snapshots
- Signal termination cleanly so memory always survives process death

If you get these five things right, the kernel gives you strong guarantees around concurrency, failure survival, and merge integrity. Everything else (your actual recon logic, fuzzers, mappers, swarm coordination) is up to you.

---

## The Minimal Surface You Must Implement

Any compliant harness, no matter how simple or sophisticated, must own these responsibilities. They map directly to the triad.

### 1. Worktree Lifecycle (from isolation-routing.md)
- Create a fresh, cryptographically isolated worktree for each worker instance.
- Mount only a read-only, hash-verified snapshot of the minimum verified state the worker needs.
- Never allow the worker to touch the live `wiki/` or `Human/` trees.
- Tear down the worktree completely on termination (success or failure).

### 2. `.ktrans` Emission (from transactional-memory.md + isolation-routing.md)
- Support **micro-transactions**: emit a `.ktrans` as soon as a coherent, provenance-backed observation or inference is ready — even if the overall task is still running.
- On **any termination path** (graceful exit, panic, explicit SIGKILL for doom-loop, external signal), ensure a final `.ktrans` is flushed that captures the state of the failure.
- For doom-loop terminations, the final transaction must include the loop signature and token-velocity profile.
- Every `.ktrans` must carry at minimum: `tx_id` (UUIDv7), `worker_id`, `routing_id`, `base_snapshot`, `provenance_chain`, and the `mutations` array.

### 3. STALLED / Routing Liveness Handling (from isolation-routing.md)
- Participate in the Leader-Broker acknowledgment protocol.
- If a routing payload is not acknowledged within the epoch window, surface the `STALLED` status.
- Turn `STALLED` events into first-class artifacts so dependent claims can be contested or re-verified.

### 4. Provenance & Snapshot Discipline
- Never let a worker start from a stale or unverified base without recording the `base_snapshot` hash.
- Propagate provenance through every `.ktrans` so the Broker can perform accurate three-way rebasement later.

### 5. Clean Termination Signaling
- When a worker is killed (especially by the Broker for doom-loop reasons), the harness must still deliver the final `.ktrans`.
- The kernel treats “the process died” as a normal (if unfortunate) termination path, not a special case that loses data.

Get these five right and the kernel’s protections activate automatically.

---

## Two Common Harness Patterns

### Pattern A: Single-Worker Harness (TUI / CLI Driver Style)
This is the style the original Grok Build TUI used.

**Characteristics:**
- One primary worker at a time (or a small number of coordinated workers)
- The harness often doubles as the user interface or command driver
- Simpler routing — the “Leader” and “Broker” roles can live in the same process or be lightly separated

**Typical responsibilities in this pattern:**
- The harness itself manages the worktree pool
- It directly monitors token velocity and AST delta for doom-loop detection
- It is responsible for flushing the final `.ktrans` on SIGKILL or crash
- Merge decisions often surface to a human operator (or a lightweight review pane)

**When to choose it:**
- You’re building an interactive tool (like the original Grok TUI)
- You want tight human oversight during early development
- The workload is bursty rather than sustained parallel campaigns

### Pattern B: Multi-Worker Swarm / Leader-Broker Orchestrator
This is the pattern for serious scale.

**Characteristics:**
- A central Leader plans and emits routing payloads
- Multiple independent Brokers manage pools of workers
- Workers run in complete isolation and only ever communicate via `.ktrans`
- The system is designed for long-running, high-parallelism offensive or research campaigns

**Typical responsibilities in this pattern:**
- The Leader only emits routing decisions and never touches worker state directly
- Each Broker enforces epoch windows, back-pressure, and `STALLED` handling
- Workers are truly disposable — the harness guarantees that valuable state has already left the worktree in a `.ktrans` before any kill
- Merge arbitration and rebasement happen centrally at the Broker or a dedicated merge service

**When to choose it:**
- You are building background daemons, RedMicro-style wave orchestrators, or large-scale reconnaissance swarms
- You need horizontal scaling and strong failure isolation
- You want the kernel’s concurrency and survival guarantees to do the heavy lifting

Most serious long-term use of Korg will eventually move toward Pattern B.

---

## Your First 30 Minutes (Getting Something Compliant Running)

Here is the smallest possible skeleton that satisfies the kernel contracts:

1. **Create a worktree manager**
   - On worker start: generate a unique `$WORKER_ID`, create `/tmp/korg/worktrees/$WORKER_ID`
   - Pull a minimal verified snapshot (use content hashes)

2. **Implement the emission path**
   - Build a small `.ktrans` writer that can emit both micro-updates and a final termination record
   - Make sure the writer always runs on process exit (use `atexit`, signal handlers, or a supervising shim)

3. **Wire up routing acknowledgments (minimal version)**
   - When you receive a routing payload, reply with an ack within the epoch
   - If you miss the window, emit a `STALLED` artifact before shutting down the worker

4. **Hook termination**
   - On any exit path, flush a final `.ktrans` that includes `exit_reason` and (if applicable) doom-loop diagnostics
   - Only then allow the worktree to be torn down

5. **Test the survival guarantee**
   - Kill a worker mid-run (kill -9 or simulate a doom-loop)
   - Verify that the last coherent state it discovered still appears in the vault as either a `semantic-decision` or `failed-experiment`

If you can make the above five steps work, you have a kernel-compliant harness — even if it is only 200 lines of glue.

Everything else (your actual recon logic, how you present findings, your UI or API) is now safely decoupled from memory governance.

---

## Common Pitfalls the Kernel Protects You From

| Pitfall | What usually happens without the kernel | How the Korg contracts protect you |
|---------|-----------------------------------------|------------------------------------|
| Worker dies mid-campaign | All intermediate findings are lost | Mandatory terminal `.ktrans` + micro-transaction rule means the last stable state survives |
| Two workers write conflicting facts | Last-writer-wins or silent corruption | Authority-vector merge + three-way rebasement + explicit `CONTESTED` + `semantic-decision` records |
| Worker goes into a loop | Wasted tokens and context poisoning | Doom-loop detection + Broker SIGKILL + final diagnostic `.ktrans` that documents the failure |
| Late-arriving results from a slow worker | Ignored or applied to wrong base | `base_snapshot` + headless rebasement protocol |
| Harness crashes while managing workers | Inconsistent state between workers and vault | Workers only ever write through `.ktrans`; the harness itself is not trusted with direct vault writes |

The kernel does not prevent you from making mistakes. It makes the consequences of those mistakes visible, contained, and recoverable.

---

## Where This Takes You Next

Once you have even a minimal harness running against the triad, the next high-value questions become:

- **How do I actually watch what is happening?** → [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]]
- **How do I review contested facts and make good arbitration decisions?** → [[Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md]]
- **What does a production-grade reference harness look like?** (We will publish one once the contracts stabilize further)
- **How do I bind this to ACP** so different tools can participate in the same swarm?
  - [[wiki/reference-harness/ACP-Binding-Design.md]] (minimal protocol)
  - [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] (how Leader + Brokers actually coordinate at scale)
  - [[wiki/reference-harness/ACP-Message-Schema.md]] (concrete schemas and wire format)
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] (production-grade example of these responsibilities in the wild)

The contracts are deliberately small. A good harness is mostly just disciplined glue that never lies to the kernel about what its workers have actually proven.

Build the thinnest possible layer that honors the five responsibilities above, and the rest of the system will compound knowledge for you instead of fighting you.

---

This is the practical bridge between “I understand the Korg Triad” and “I am building something real on top of it.”

The kernel is done. The harness layer is where the leverage begins.