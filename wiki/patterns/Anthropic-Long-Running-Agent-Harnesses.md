---
title: "Anthropic-Long-Running-Agent-Harnesses"
date: 2026-05-21
type: concept
tags: [pattern, long-running-agents, harness-design, adversarial-loops, verification, multi-agent, anthropic, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Pattern: Anthropic Long-Running Agent Harnesses

**A production-grade set of harness design patterns for agents that operate over many hours or days, centered on adversarial generator/evaluator loops, persistent artifacts, explicit verification contracts, and deliberate separation of roles rather than relying on model self-assessment.**

This pattern was extracted from the Anthropic Applied AI team talk (Ash + Andrew) at the AI Engineer Conference, May 2026. It represents one of the most detailed public accounts of how a frontier lab builds harnesses that can sustain long-running, high-stakes agentic work.

---

## Context

Long-running agents (multi-hour or multi-day sessions) expose fundamental weaknesses in naive “just give the model more context” approaches. Anthropic’s experience shows that harness architecture must co-evolve with model capability. The harness is not a temporary scaffold — it is the primary mechanism for quality, recovery, and context management at scale.

---

## Core Harness Patterns

- **Generator + Evaluator Adversarial Loop** (GAN-style separation)
- **Planner + Generator + Evaluator** multi-role decomposition
- **Persistent Artifacts & Checkpoints** that survive context resets
- **Verification Loops** driven by external execution rather than self-judgment
- **Agent Teams / Sub-agent Coordination** with structured inter-agent communication
- **Skills & Progressive Disclosure** for disciplined context management

---

## Key Techniques

### Generator + Evaluator Adversarial Loop

The generator (builder) and evaluator (critic) are deliberately separated. The evaluator is tuned to be harsh and is given access to live execution environments (Playwright, actual running apps, tests, screenshots) instead of only reading code diffs.

Before implementation begins, the generator and evaluator negotiate an explicit “contract” defining what “done” and “acceptable” mean. This contract becomes the rubric for final grading.

### Planner + Generator + Evaluator Pattern

- **Planner**: Decomposes vague goals into sprints and features.
- **Generator**: Implements the current sprint/feature.
- **Evaluator**: Runs the system, critiques with live interaction, and can force the team to abandon an entire approach.

This structure allows the system to throw away and restart directions when the evaluator determines the current path is failing.

### Persistent Artifacts & Checkpoints

File-system artifacts (JSON feature lists, progress trackers, git commits, structured logs) are preferred over markdown because models are less likely to corrupt or overwrite them during context resets. These artifacts enable safe rewinding and recovery across long sessions.

### Verification & Self-Judgment

Models are poor at judging their own output. The harness compensates by using a dedicated, harshly-tuned evaluator that actually executes the application rather than trusting the generator’s self-assessment.

### Context Management

Progressive disclosure through Skills, server-side compaction, and structured handoffs between agents. Fresh context windows are used strategically instead of attempting to maintain a single ever-growing session.

### Agent Teams

Multiple specialized agents communicate through defined protocols. Sub-agents can be spawned, monitored, and terminated independently while contributing to a shared persistent state.

---

## Concrete Examples from the Talk

- A retro game maker built using the generator/evaluator pattern that produced significantly more complete and functional results than a single-agent run.
- Full-stack web applications developed over 5–6+ hour continuous sessions using the Planner + Generator + Evaluator structure with persistent checkpoints.

---

## Mappings to Korg

| Anthropic Pattern                     | Korg Equivalent                                      | Strength of Fit |
|---------------------------------------|------------------------------------------------------|-----------------|
| Generator + Evaluator adversarial loop | Arena Mode / Merge-Arbitration Engine                | Extremely High |
| Persistent artifacts & checkpoints     | Blackboard + `.ktrans` + transactional memory        | Extremely High |
| Planner + Generator + Evaluator        | Leader + Worker personas + epistemic state machine   | Very High |
| Agent teams / sub-agent communication  | Leader-Broker-ACP model                              | Very High |
| Verification loops (external execution) | Epistemic State Machine (VERIFIED criteria)         | High |
| Skills & progressive disclosure        | Delta patching + blackboard + capability model       | High |
| Reading traces by hand                 | How-to-Watch-a-Live-Campaign operator guidance       | High |

---

## Lessons for Harness Design

- Harnesses and models co-evolve. As models improve, certain harness components can be simplified or removed, but the core separation of concerns (especially adversarial evaluation) tends to remain valuable.
- Separate context windows + deliberate adversarial pressure are more powerful than raw model scale for long-running work.
- Self-evaluation is a fundamental trap. A dedicated, harshly-tuned evaluator using external verification consistently outperforms the builder judging its own work.
- Taste and subjective quality can be graded reliably when a strong, explicit rubric is written down in advance.
- The highest-value debugging often comes from deeply reading execution traces rather than running additional experiments.

---

## Related

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground-truth architecture for the Leader-Broker-Worker model and Arena Mode.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Concrete pseudocode for the LeaderOrchestrator and 4-persona dispatch that aligns with the Planner + Generator + Evaluator pattern.
- [[Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md]] — Operator guidance for monitoring long-running multi-agent systems.
- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine and Merge-Arbitration Engine.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` and persistent artifact mechanics.
- [[wiki/patterns/Cross-Harness-Pattern-Extraction.md]] — The broader methodology for turning external harness practices into Korg patterns.

*This note was created as a cross-harness pattern extraction on 2026-05-20.*

## See Also

- [[Synthesis — Adversarial Loop]]

- [[Synthesis — Transactional Memory]]

- [[Synthesis — Blackboard]]


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: medium)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
