---
date: 2026-05-20
title: How to Watch a Live 16-Agent Campaign
type: methodology
tags: [methodology, monitoring, observability, grok-heavy, 16-agent, operator-experience, grok-build]
harness: korg
domain: operations, monitoring, heavy-swarm
---

# How to Watch a Live 16-Agent Campaign

Once you have a harness (or Grok Build itself) running against the real Grok 4.20 Heavy architecture, the question changes from “is it working?” to **“is the swarm healthy, or is it quietly burning tokens and accumulating contested facts?”**

A 16-agent Heavy campaign is not just “more agents.” It is a living, self-scoring, multi-persona debate system running on a shared ~3T MoE backbone with aggressive KV reuse, token-bucket gating, and mandatory human gates at key moments. Watching it correctly means reading the **epistemic health** of the swarm, not just its CPU or token count.

This note is the operator companion to the reference-harness layer. It assumes you have read the ground-truth architecture and the three Korg mechanism contracts.

---

## The 4-Persona Lens (Captain, Harper, Benjamin, Lucas)

In practice, almost every Heavy campaign organizes around four recurring specialized heads (they can be replicated or further specialized when you scale to 16 agents):

| Persona     | Primary Role                     | What “Healthy” Looks Like                              | Early Trouble Signs                              |
|-------------|----------------------------------|---------------------------------------------------------|--------------------------------------------------|
| **Captain / Grok** | Root planning, synthesis, final authority | Issues clear PlanPresentation events; surfaces ranked options with confidence | Vague plans, repeated re-planning, low confidence in ApprovalRequests |
| **Harper**  | Critique, research, evidence     | High volume of well-provenanced micro-`.ktrans`; surfaces counter-evidence quickly | Many low-confidence inferences; frequent CONTESTED artifacts |
| **Benjamin**| Tool use, execution, file work   | Clean worktree lifecycle; terminal `.ktrans` on every exit (even SIGKILL) | Worktree bloat, missing terminal transactions, repeated STALLED events |
| **Lucas**   | Orchestration, cross-agent merge | Quickly proposes hybrids in Arena; keeps blackboard contention low | Slow conflict resolution; growing backlog of contested claims |

**Rule of thumb:** In a healthy 4-agent core (and its 16-agent expansion), you see all four personas producing useful signal roughly in balance. One persona dominating usually means the task decomposition was too narrow.

---

## The Signals That Actually Matter in a Heavy Swarm

These are the high-signal streams you should have visible (via CLI, TUI, or blackboard queries). Everything else is noise.

### 1. Token Velocity vs. Effective Burn Rate
- Watch the rolling token velocity against the session’s token-bucket (see `Token-Bucket-Throttling-and-Resource-Gating.md`).
- **Healthy:** Velocity rises during exploration then smoothly declines as the swarm converges. The EffectiveBurnRate stays well inside the warning zone.
- **Trouble:** Sustained high velocity with flat semantic progress (especially when the bucket is already <40%). This is the classic pre-doom-loop pattern.
- **Action:** Look for the Broker already applying velocity throttling or soft walls. If you see repeated “reduce_max_agents” soft walls, the campaign is under resource pressure.

### 2. Arena Self-Score Vectors & Aggregation
- Every contested result should produce visible self-scoring vectors from the participating agents (correctness, completeness, novelty, minimal_diff, provenance_strength).
- **Healthy:** Scores are spread out; the Leader’s weighted aggregation produces a clear winner or a well-justified hybrid within 1–2 Arena rounds.
- **Trouble:** All agents giving themselves 0.9+ on every dimension, or the Leader repeatedly choosing “hybrid” because no clear winner emerges.
- In Grok Build you will see these scores in the ApprovalRequest screen. Watch for suspiciously uniform high scores — that is often a sign of groupthink or weak critique from Harper.

### 3. Blackboard Contention & Rebasement Pressure
- Track writes that go through `PENDING_ARBITRATION` or require three-way rebasement.
- **Healthy:** Occasional contention that resolves quickly (usually within one or two human review cycles).
- **Trouble:** Steadily rising number of contested facts, especially when low-authority inferences keep fighting high-authority static analysis or prior VERIFIED claims.
- The terminal `.ktrans` from each worker is your best audit trail here.

### 4. PlanPresentation and ApprovalRequest Events
- These are the explicit human-in-the-loop moments.
- **Healthy campaign:** PlanPresentation appears early, is reasonably scoped, and receives a thoughtful `task.approve` (possibly with light edits). A later ApprovalRequest shows clear ranked candidates with Arena scores.
- **Trouble:** Repeated PlanPresentation events (the swarm is thrashing on scope), or ApprovalRequests that keep getting `task.reject` because the candidates are low-value or over-engineered.
- In a live Grok Build session you can watch these events surface directly in the TUI.

### 5. Worker Lifecycle & Productive Death Ratio
- Graceful exits : SIGKILL (doom-loop) : crash : STALLED timeout.
- **Healthy:** Most workers deliver a rich terminal `.ktrans` even when killed. You see “productive death” — the worker contributed real epistemic work before termination.
- **Trouble:** High percentage of silent or thin terminations. Workers disappearing without a final transaction is a red flag that your harness is not obeying the transactional-memory contract.

### 6. Session Recovery & Checkpoint Health
- If you resume a session (`--session <id>`), watch the replay behavior.
- **Healthy:** Quick replay of the last verified checkpoint, then smooth re-dispatch of only the still-pending RouteWork items.
- **Trouble:** Long replay times, many re-dispatches of the same work, or worktrees that can no longer be re-attached cleanly.

---

## Healthy 16-Agent Campaign vs. One Heading for Trouble

**A healthy Heavy swarm looks like this:**
- All four core personas (and their scaled copies) are producing distinct, high-provenance signal.
- Token velocity follows a clear “explore → debate → converge” curve inside the token-bucket budget.
- Arena rounds are short and decisive; hybrids are rare and well-justified.
- PlanPresentation and ApprovalRequest events feel like genuine collaboration points, not interruptions.
- The blackboard grows steadily with VERIFIED facts; CONTESTED backlog stays small and is actively reviewed.
- When workers die (including forced kills), they leave behind useful terminal transactions.

**A campaign drifting into trouble looks like this:**
- One or two personas dominate while others go quiet.
- Token bucket is repeatedly hitting warning/critical zones with little semantic progress.
- Arena self-scores are uniformly high or the Leader keeps punting to “hybrid” or human escalation.
- You see clusters of STALLED events or repeated PlanPresentation without forward movement.
- The CONTESTED folder (or equivalent) is growing faster than the operator or the Merge-Arbitration Engine can clear it.
- Many workers are terminating without rich terminal `.ktrans`.

---

## When and How to Intervene

The architecture is deliberately designed with explicit human gates. Use them.

- **At PlanPresentation:** Edit scope aggressively if the proposed DAG is too broad or too shallow. This is the highest-leverage intervention point.
- **At ApprovalRequest:** Reject low-value or over-confident candidates. Use the `authority_override` field only when you have strong reason to trust one line of work over the Arena scores.
- **Mid-campaign via `capability.revoke`:** If you see one persona (e.g., an over-eager Benjamin) causing repeated conflicts, you can revoke its write or tool capabilities for the rest of the epoch.
- **Session recovery:** If the swarm is clearly in a bad state, terminate the session and resume from the last solid checkpoint rather than letting it continue thrashing.

**Rule of thumb:** Intervene early at the plan or first ApprovalRequest stage. Late intervention (after dozens of contested facts have accumulated) is much more expensive.

---

## Practical Watching Workflow (Grok Build + Custom Harness)

1. Keep the main Grok Build TUI (or your harness equivalent) visible for PlanPresentation and ApprovalRequest events.
2. Have a side terminal or dashboard showing:
   - Current EffectiveBurnRate and bucket remaining
   - Live count of CONTESTED artifacts
   - Recent terminal `.ktrans` (especially from Harper and Benjamin)
   - Arena score distribution from the last 1–2 rounds
3. Periodically query the blackboard for provenance chains on key claims.
4. When you see a worker die, immediately look at its terminal transaction. That is often more diagnostic than the live output.

---

## Closing

Watching a live 16-agent Heavy campaign is an exercise in **epistemic situational awareness**. You are not babysitting four (or sixteen) separate agents — you are watching a shared-memory debate system with strong self-critique and explicit human veto points.

The best operators spend most of their time in a calm monitoring state, only stepping in at the two or three high-signal human gates the architecture deliberately surfaces. When the signals above look healthy, trust the Merge-Arbitration Engine and the token-bucket governor. When they don’t, use the ACP control messages (`task.approve`, `task.reject`, `capability.revoke`) early.

This note, together with the reference-harness pseudocode and the ground-truth architecture, gives you the complete “Understand → Build → Operate” loop for working with real Grok 4.20 Heavy swarms.

---

## Related

- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — Complementary practitioner patterns for long-running agent systems, especially the emphasis on dedicated harsh evaluators, persistent artifacts, and reading traces by hand.

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — The ground-truth architecture (especially the 4-persona topology and Arena sections).
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — The more abstract, harness-agnostic version of this guide.
- [[wiki/mechanisms/state-primitives.md]] — Epistemic states and the Merge-Arbitration Engine you are watching.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — The concrete PlanPresentation / ApprovalRequest / recovery flows you will see in the UI.
- [[wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md]] — The resource signals that keep the swarm from eating the cluster.