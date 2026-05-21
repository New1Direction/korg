---
date: 2026-05-20
title: The Korg Triad
type: methodology
tags: [methodology, overview, triad, headless]
---

# The Korg Triad

**The complete, closed system that lets any headless worker safely interact with a governed, transactional memory layer.**

Most agent systems treat memory as a side effect.  
Korg treats it as a first-class, protected, queryable artifact with clear rules for how it changes — even when the agents themselves are dying, stalling, or conflicting with each other.

The three mechanism notes in `wiki/mechanisms/` are not three separate ideas. They are three sides of one operating system for agentic work.

---

## The Three Pillars

### 1. Epistemic State Machine (`state-primitives.md`)

This is the *lifecycle* every piece of knowledge goes through.

An observation doesn’t jump straight from “the tool said so” to “we now believe this is true in the vault.” It moves deliberately:

**OBSERVED → INFERRED → VERIFIED → CONTESTED**

The transition to `VERIFIED` is the important gate. It only happens when something deterministic and strong happens — a cryptographic hash matches, multiple independent tools agree, or a higher-authority source stamps it.

The `CONTESTED` state is not a failure. It is how the system stays honest. When new evidence contradicts something previously trusted, the fact is moved aside rather than silently overwritten. This is the immune system of the memory layer.

**Why it matters operationally**  
Without this machine, you end up with a growing pile of “probably true” statements that slowly become untrustworthy. With it, every claim carries a visible, queryable history of how sure we are and why.

### 2. Leader-Broker Routing & Worktree Isolation (`isolation-routing.md`)

This is the *physical protection layer*.

When you have multiple workers running at the same time (one mapping auth, one fuzzing SSRF, one doing deep recon), they cannot be allowed to step on each other or on the live vault.

The contract is simple but strict:

- Every worker runs inside its own ephemeral, isolated workspace (`/tmp/korg/worktrees/...`).
- It never touches the real `wiki/` or `Human/` directories.
- It is only allowed to emit **one** thing when it finishes (or dies): a `.ktrans` transaction file.
- The Broker watches routing acknowledgments. If a worker goes silent too long, it gets marked `STALLED` and the system moves on instead of hanging forever.
- Back-pressure is differentiated: noisy low-value telemetry can be dropped. High-value structural claims will block the worker until the system can safely accept them.

**Why it matters operationally**  
This is what lets you run aggressive parallel offensive work without the entire job corrupting the memory it was trying to improve. The workers can be killed (even for doom-loop reasons) and the vault stays clean.

### 3. Transactional Memory Serialization (`.ktrans`) (`transactional-memory.md`)

This is the *handoff contract* — the actual thing that moves between the isolated worker and the protected vault.

A `.ktrans` file is small, self-describing, and deliberately limited. It contains:

- A time-sortable ID (`tx_id` as UUIDv7)
- Exactly which worker produced it and which routing decision authorized the work
- The snapshot it started from (so staleness can be detected)
- A list of proposed mutations (`INSERT`, `UPDATE`, or `CONTEST`)
- Full provenance so you can later ask “why did we believe this?”

The most important property is the **decoupling of code rollback from memory persistence**.

If a worker is killed (SIGKILL for a doom loop, panic, OOM, whatever), its local process state disappears. But the last coherent `.ktrans` it managed to emit does not. The findings that were solid enough to write down survive the death of the process that discovered them.

When a `.ktrans` arrives late, the system doesn’t just accept or reject it. It performs a headless three-way semantic rebase against whatever has happened in the meantime, respecting the authority vector of the changes. Higher-authority facts win. Equal-authority conflicts become first-class `semantic-decision` records instead of silent corruption.

**Why it matters operationally**  
This is the difference between “we lost the last four hours of work when the fuzzer crashed” and “we have a clean, auditable record of exactly what that worker had proven before it died, and the system resolved the conflicts intelligently.”

---

## How the Three Pieces Work Together (A Worker Lifecycle)

1. **Spawn** — The Leader emits a routing payload to a Broker for a specific capability.
2. **Isolate** — The Broker gives the worker a fresh, read-only snapshot and its own isolated worktree. No access to the live vault.
3. **Execute** — The worker runs. As it discovers things worth remembering, it periodically emits `.ktrans` micro-transactions (not just at the end).
4. **Terminate** (even badly) — Whether the worker finishes cleanly, hits a doom loop and gets SIGKILLed, or crashes, the last `.ktrans` it produced is delivered to the Broker.
5. **Rebase & Merge** — The Broker sees the transaction. If the base is stale, it does the three-way semantic rebase. Authority rules are applied. Conflicts become explicit `CONTESTED` facts + decision records.
6. **Promote or Contest** — Validated mutations enter the Epistemic State Machine. Strong evidence can move facts to `VERIFIED`. Contradictions move things to `CONTESTED` and trigger further investigation.

At no point can a misbehaving or unlucky worker corrupt the central memory.

---

## What It Feels Like to Build Against This Kernel

As a harness author or tool builder, you no longer have to invent your own memory model every time.

You get a small, clear contract:

- “Here is how I must emit state changes (` .ktrans` with this shape).”
- “Here is the guarantee that my process can die without losing what I’ve already proven.”
- “Here is how conflicts with other workers will be resolved (authority vectors, not last-writer-wins).”
- “Here is how the system will notice if I go quiet or start looping.”

You stop worrying about “what if two of my workers disagree?” or “what happens when this long-running job gets killed?” The kernel handles it deterministically and records the reasoning.

This is the difference between a clever script and a system that compounds knowledge across months of aggressive, parallel, sometimes-failing work.

---

## What Comes Next

With the triad in place, the interesting work shifts to:

- Building the first real reference harness that actually implements these contracts (headless CLI worker, background daemon, or swarm coordinator).
- Defining the ACP (Agent Client Protocol) bindings so different tools can speak the same routing and transaction language.
- Adding the measurement layer (how many transactions per second, rebasement frequency, authority distribution, doom-loop frequency, etc.).
- Writing the operator playbooks (“How do I watch a campaign in progress?”, “How do I review contested facts?”, “How do I trust the vault after a week of heavy parallel work?”).

See the follow-up note [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] for concrete guidance on what a harness author must actually implement.

Once you have harnesses running, [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] shows you exactly what signals to watch and how to know when a campaign is healthy versus heading for trouble.

When contested facts appear, [[Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md]] gives the practical workflow for reviewing and resolving them.

The technical specification is done. The living room — the part that makes the system feel usable and powerful to actual humans and harness builders — is what we build now.

---

This is the foundation. Everything else (skills, harnesses, long-running campaigns, the `Human/Methodology/` layer itself) will be built on top of these three ideas.

The memory layer finally has rules that survive the chaos of real agentic work.