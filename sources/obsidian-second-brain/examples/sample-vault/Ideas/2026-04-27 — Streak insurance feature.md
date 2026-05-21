---
date: 2026-04-27
type: idea
tags: [idea, tide, retention, monetization]
status: captured
related-projects: ["[[Projects/Tide]]"]
ai-first: true
---

# Streak insurance feature

## For future Claude

Idea captured on 2026-04-27 about a "streak insurance" feature for [[Projects/Tide]]. Status: captured. The body explains the idea, why it's interesting, and what would make it real. If shelved later, the reason will be documented at the bottom. Owner: [[people/Alex Rivera]].

## The idea

Let users buy or earn "streak insurance" days that absorb missed habits without breaking the tide level. Two variants worth exploring:

- **Earned variant:** users earn one insurance day per 14 consecutive days of activity, capped at 3 in the bank
- **Paid variant:** Tide Plus subscribers get 5 insurance days per month, automatically applied

## Why it's interesting

- Solves the "I missed one day on vacation, lost my whole streak" complaint that drove the [[Projects/Tide]] retention rebuild
- Earned variant reinforces consistent users without changing pricing
- Paid variant gives Tide Plus a tangible "save" moment, which converts better than abstract benefits in habit-app pricing pages (confidence: `medium`, single-source observation, TBD: cite habit-app pricing study)

## What would make it real

- Decide: feature inside [[Projects/Tide]] or its own product entirely?
- Mockup the "insurance applied" UI moment (the apology-receipt language matters)
- Validate with 5 paying users before building (`stated` interview required)
- Database: extend the `streaks` table with `insurance_balance` and `insurance_applied_at` columns

## Risks

- Could feel gamey, which contradicts Tide's calmer-streak positioning. Counter: framing as "rest credit" rather than "insurance" might fix the tone (confidence: `speculation`).
- Earned variant complicates the tide-level math that just got rebuilt this week

## Next step

Promote to a feature spec inside [[Projects/Tide]] if the rebuild ships clean and user feedback on the calmer-streak framing comes back positive. Decision date: TBD, expected post-2026-05-08 (v0.9.0 ship).

## Sources

- Origin: pair session with [[people/Sam Patel]] on 2026-04-27, see [[wiki/logs/2026-04-27 — Tide retention rebuild]]
- Adjacent: TBD competitor research on rest mechanics
