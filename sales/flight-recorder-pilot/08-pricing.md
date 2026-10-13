# Pricing options

All prices are fixed, in USD, exclusive of taxes, for remote delivery. Scope per tier
is in [03-scope-and-exclusions.md](03-scope-and-exclusions.md); the canonical
summary is in [README.md](README.md#pilot-at-a-glance).

## Tiers

### Recorder — $6,500 · 2 weeks

For a team that wants to know whether Claude Code sessions can be recorded,
redacted, and independently verified on its own machines.

- 1 repository, up to 5 pilot developers on Claude Code
- Capture, verification kit, customer-held issuer key, sample Certificates
- Tamper, wrong-key, and redaction-canary drills
- Coverage and overhead measurement
- Findings report and 60-minute readout
- 14 days of async support after acceptance

### Recorder + CI Gate — $14,500 · 3 weeks *(recommended)*

For a team that wants verification wired into pull requests, so reviewers stop
taking an agent transcript on trust.

Everything in Recorder, plus:

- Up to 2 repositories, up to 10 pilot developers
- `verify-korgcert` GitHub Action pinned to the customer's issuer key, proven with
  one passing PR and one deliberately failing PR
- Public git-tip anchoring demonstration (opt-in; publishes a hash only) or
  structural-only memo
- Codex CLI feasibility replay of one sanitized trace
- 30 days of async support after acceptance

### Evidence Pack — $24,000 · 4 weeks

For a team with an audit, enterprise deal, or board question on the calendar,
which needs the pilot output in a form its GRC owner can take to an auditor.

Everything in Recorder + CI Gate, plus:

- Up to 3 repositories, up to 20 pilot developers
- Half-day control-evidence mapping workshop, producing a draft evidence matrix
  for the customer's own selected controls (not an audit opinion)
- Operating runbook: rollout, key custody and rotation, retention, redaction
  incidents, verifier upgrades
- Executive readout for a second audience
- 30 days of async support after acceptance

## Comparison

| | Recorder | Recorder + CI Gate | Evidence Pack |
|---|---|---|---|
| Price | $6,500 | $14,500 | $24,000 |
| Duration | 2 weeks | 3 weeks | 4 weeks |
| Repositories | 1 | up to 2 | up to 3 |
| Pilot developers | up to 5 | up to 10 | up to 20 |
| Deliverables | D1–D9 | D1–D12 | D1–D15 |
| Support after acceptance | 14 days | 30 days | 30 days |

Deliverable numbers refer to [04-deliverables.md](04-deliverables.md).

## Payment terms

- **50% at signature**, invoiced with the SOW and payable before kickoff.
- **50% on acceptance**, invoiced when the pilot is accepted under
  [09-acceptance-criteria.md](09-acceptance-criteria.md#acceptance-process),
  payable net 15.
- **Fit check and early exit.** If the discovery questionnaire shows Korg cannot capture the
  customer's agents, we decline before any invoice. If a hard blocker that was
  not disclosed in discovery appears in the first three business days after
  kickoff (for example, hooks blocked by policy with no exception), the customer
  may cancel and we refund the first payment minus $1,500 for work performed.
- **Pause.** If the customer pauses the pilot for more than 15 business days, the
  remaining work is rescheduled by mutual agreement. A pause longer than 60
  business days ends the pilot. In that case we invoice delivered work pro rata,
  capped at the remaining balance.

## Change orders

Work outside the selected tier's scope is quoted in writing before it starts:

| Item | Price |
|---|---|
| Additional repository (within the same CI setup) | $1,500 each |
| Additional 5 pilot developers | $1,500 per block |
| Additional pilot week (more capture and measurement time) | $3,500 |
| Upgrade between tiers during the pilot | Difference in price; timeline extends to the higher tier's duration |
| Other out-of-scope work | $2,000 per day, quoted as a fixed amount before work starts |

## Discounts

Discounts never change scope or acceptance criteria.

- **Design partner:** 20% off any tier if the customer agrees to a named
  case study or a public reference quote after acceptance, with the customer's
  approval of every word.
- **Upfront payment:** 5% off for paying 100% at signature.
- Only one discount applies.

## Price guidance for sellers

- **Lead with Recorder + CI Gate.** It is the smallest tier that changes a
  reviewer's daily workflow (a pass or fail on the PR), which is the strongest
  rollout argument.
- **Offer Recorder** when the buyer needs a low-risk first step or has no GitHub
  Actions.
- **Offer Evidence Pack** only when there is a dated audit, deal, or board ask, and
  only with the "draft for your auditor, not an audit" framing.
- Prices are a starting point for the first pilots and should be revisited after the
  first three closed deals, based on actual effort.
