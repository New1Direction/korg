---
date: 2026-05-20
title: How to Watch a Live Campaign
type: methodology
tags: [methodology, monitoring, observability, operator-experience, campaign]
---

# How to Watch a Live Campaign

Once you have a harness running against the Korg kernel, the next question is:  
**What should I actually be looking at while the system is working?**

Watching a live campaign in Korg is not the same as watching a normal agent run. You are not just looking at tool output or logs. You are watching the **epistemic lifecycle in motion** — artifacts moving through states, workers being born and killed, transactions being emitted and rebased, and the Merge-Arbitration Engine quietly protecting the long-term integrity of the vault.

A good operator does not stare at raw telemetry. They watch a small number of high-signal streams that tell them whether the system is compounding knowledge or heading toward waste and corruption.

For operators working specifically with Grok 4.20 Heavy / 16-agent swarms (Grok Build or equivalent harnesses), see the companion note [[Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md]] which adds the 4-persona lens (Captain, Harper, Benjamin, Lucas), Arena self-scoring, token-bucket signals, and Heavy-specific intervention patterns.

---

## What “Watching” Actually Means

In the Korg model, watching a campaign means tracking four overlapping layers at once:

1. **Epistemic flow** — Are new facts moving from `OBSERVED` → `INFERRED` → `VERIFIED`, or are they piling up as `CONTESTED`?
2. **Worker health & liveness** — Are workers making progress, stalling, looping, or dying productively?
3. **Transaction throughput** — Is the system generating useful `.ktrans` traffic, or is it mostly noise?
4. **Merge & arbitration pressure** — Is the Broker handling conflicts gracefully, or are we accumulating contested facts that need human attention?

The kernel and a well-written harness surface these signals cleanly. Your job as an operator is to know which ones matter at any given moment.

---

## The Key Signals You Should Actually Care About

These are the live observables that experienced operators watch. Everything else is secondary.

### 1. Token Velocity + AST Delta (Doom-Loop Radar)
- **Why it matters:** This is the earliest warning that a worker (or the whole campaign) is spinning.
- **Healthy signal:** Steady, gradually decreasing velocity as the worker covers new ground.
- **Trouble signal:** High velocity with near-zero AST/semantic change for several cycles.
- **Action:** The Broker should already be killing these workers. You watch to see *how often* it is happening and whether the same attack surface keeps triggering loops.

### 2. `.ktrans` Emission Rate & Quality
- **Micro-transactions per minute** — Are workers emitting useful state as they discover it, or only dumping everything at the end?
- **Terminal transaction ratio** — What percentage of workers are producing a proper final `.ktrans` when they die (especially on SIGKILL)?
- **Provenance richness** — Are the transactions carrying real upstream hashes and routing IDs, or are they thin?

**Rule of thumb:** A healthy campaign has a steady stream of small, high-provenance micro-transactions rather than rare, fat ones.

### 3. STALLED Events
- Every `STALLED` routing acknowledgment failure is a first-class signal.
- Watch the rate and the pattern. A few scattered STALLED events are normal in a large swarm. A sudden cluster usually means either:
  - A Broker is overloaded, or
  - A particular class of work is taking longer than the epoch window allows.

STALLED events should trigger automatic contestation or re-verification of any dependent claims.

### 4. Merge & Contest Pressure
- **Number of facts entering the CONTESTED holding area per hour**
- **Rebasement rate** (how often late `.ktrans` payloads require three-way merge work)
- **Authority distribution** in contested cases (are low-authority inferences constantly fighting high-authority static analysis results?)

A healthy campaign has occasional contests that get resolved quickly. A troubled one has a steadily growing backlog of `CONTESTED` artifacts that no one is looking at.

### 5. Worker Lifecycle Distribution
- How many workers are currently alive vs. terminated?
- What is the ratio of graceful exits : SIGKILL (doom-loop) : crash : STALLED timeout?
- Are you seeing “productive deaths” (workers that delivered a rich final `.ktrans` before being killed)?

---

## Healthy Campaign vs. Campaign Heading for Trouble

**Healthy looks like this:**
- Steady, moderate token velocity with clear progress (new attack surface being mapped)
- Regular micro-`.ktrans` emissions with rich provenance
- Occasional `STALLED` or doom-loop kills, but each one produces a useful diagnostic transaction
- Merge pressure is low to moderate; contested facts are being reviewed or automatically resolved within hours
- The operator mostly watches and only intervenes for high-value arbitration decisions

**Trouble looks like this:**
- Token velocity stays high while AST delta collapses (doom-loop storm)
- Most workers are only emitting one big `.ktrans` at the very end (or none at all when killed)
- `STALLED` events are clustering on the same work types
- The `CONTESTED` queue is growing faster than it is being cleared
- The operator is constantly firefighting instead of making strategic decisions

The moment you start seeing the second pattern, it is usually time to throttle the campaign, relax some authority rules, or look at the current set of contested facts.

---

## The Operator’s Toolkit — When and How to Intervene

The kernel is designed so that most of the hard work happens automatically. A good operator intervenes at the right moments rather than constantly.

### Let the System Work
- Most `CONTESTED` facts can wait for the Merge-Arbitration Engine and authority vectors to do their job.
- Most doom-loop workers should simply be killed and their final `.ktrans` recorded. Do not try to rescue them.

### Intervene When…
- A cluster of high-value attack surface is generating repeated contests between roughly equal-authority sources.
- The same worker type keeps hitting doom-loops (suggests a bad prompt, bad tool, or bad scoping).
- STALLED events are concentrated on one Broker or one class of work (capacity or epoch tuning needed).
- You see a sudden spike in low-authority inferences being contested by high-authority static analysis (the system is correctly protecting you — you may just want to understand why).

### Typical Interventions
- Pause or reduce parallelism on a specific attack surface
- Trigger a manual rebase or boundary relaxation on a contested cluster
- Adjust epoch windows or back-pressure thresholds on a Broker
- Promote a particularly strong set of findings to `VERIFIED` by hand (rare, but sometimes correct)
- Kill an entire class of workers that have become unproductive

The golden rule: intervene at the level of **strategy and capacity**, not at the level of individual facts.

---

## How the Broker and Leader Surface These Signals

A well-designed harness + Broker makes the above observables easy to watch:

- **Bounded agent graph view** — Shows live workers, their current routing payloads, and recent `.ktrans` emission rates.
- **Provenance chain browser** — Lets you click any fact and walk backward through the exact sequence of workers and transactions that produced it.
- **Velocity + delta dashboards** — Real-time plots of token velocity and structural change per worker (or per attack surface).
- **Contested queue** — Prioritized list of `CONTESTED` facts with authority vector comparison and suggested resolution.
- **Lifecycle event stream** — Clean feed of worker births, `STALLED` events, doom-loop kills, and successful rebasements.

You do not need to watch raw logs. You watch these synthesized views. The kernel’s job is to make the important things obvious and the unimportant things quiet.

---

## Where This Takes You Next

Once you are comfortable watching campaigns in real time, the natural next questions are:

- What metrics should a production harness actually export? (token velocity histograms, rebasement latency, authority conflict rates, etc.)
- How do we build a persistent “campaign memory” on top of the live signals?
- How does ACP make it possible for completely different harnesses to participate in the same observable swarm?
  - [[wiki/reference-harness/ACP-Binding-Design.md]] (protocol)
  - [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] (Leader-Broker coordination model)
  - [[wiki/reference-harness/ACP-Message-Schema.md]] (concrete schemas)
- What does a real operator dashboard look like when you have dozens of workers across multiple attack surfaces?

The contracts give you the raw material. Watching live campaigns teaches you the *feel* of the system. That feel is what lets you design better harnesses and better interfaces later.

---

A live Korg campaign should feel like a well-instrumented scientific experiment, not a chaotic swarm of agents. You see the flow of evidence, you see where it gets stuck, and you see the system protecting its own long-term memory while the chaos happens.

That is what good observability looks like when the kernel is doing its job.

---

## Related

- [[Human/Methodology/The-Korg-Triad.md]] — The foundation this monitoring layer sits on.
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — What the harnesses you are watching must actually implement.
- [[wiki/mechanisms/state-primitives.md]] — The epistemic states whose live transitions you are observing.
- [[wiki/mechanisms/isolation-routing.md]] — The routing, STALLED, and back-pressure signals that surface in real time.
- [[wiki/mechanisms/transactional-memory.md]] — The `.ktrans` traffic, rebasement events, and contest outcomes that drive most operator decisions.
- [[Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md]] — The detailed operator workflow for the contested facts and merge pressure you will see in the live views.

This note completes the first three-piece arc of the Human/Methodology/ layer.