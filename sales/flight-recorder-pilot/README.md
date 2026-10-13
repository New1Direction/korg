# Korg Agent Flight-Recorder Pilot — sales kit

A fixed-price, fixed-scope pilot that installs Korg's open-source flight recorder
for a team's coding agents, proves on the team's own sessions that the record is
complete enough to be useful and tamper-evident, and hands over an evidence pack
the team's security, GRC, or audit owners can inspect themselves.

> **What this is not.** Korg is not a compliance certification, an audit opinion,
> or a legal assessment. The pilot produces *evidence* (verifiable records of what
> agents did) that a customer and its auditors may choose to use. Nothing in this
> kit says or implies that a customer becomes "compliant" or "certified" with any
> framework.

This directory is sales material only. It does not change Korg's source, spec, or
release artifacts; every technical statement here is traced to public code and
docs in [CLAIMS.md](CLAIMS.md).

## Pilot at a glance

These are the canonical numbers. Every other document in this kit must match them.

| | **Recorder** | **Recorder + CI Gate** | **Evidence Pack** |
|---|---|---|---|
| Fixed price (USD) | **$6,500** | **$14,500** | **$24,000** |
| Duration (after kickoff) | 2 weeks | 3 weeks | 4 weeks |
| Repositories | 1 | up to 2 | up to 3 |
| Pilot developers on Claude Code | up to 5 | up to 10 | up to 20 |
| Live capture | Claude Code | Claude Code | Claude Code |
| Certificates + pinned issuer key | ✓ | ✓ | ✓ |
| Tamper and redaction drills | ✓ | ✓ | ✓ |
| CI verification gate on pull requests | — | ✓ | ✓ |
| Public git-tip anchoring (opt-in) | — | ✓ | ✓ |
| Second-agent feasibility (Codex CLI trace replay) | — | ✓ | ✓ |
| Control-evidence mapping workshop | — | — | ✓ |
| Post-acceptance async support | 14 days | 30 days | 30 days |

Payment: 50% at signature, 50% on acceptance. Details in [08-pricing.md](08-pricing.md).

## The kit

| # | Document | Use it to |
|---|---|---|
| 01 | [Service offer](01-offer.md) | Send as the one-page offer after a first call |
| 02 | [Ideal customer profile](02-ideal-customer-profile.md) | Qualify or disqualify a prospect |
| 03 | [Scope and exclusions](03-scope-and-exclusions.md) | Attach to the SOW |
| 04 | [Deliverables](04-deliverables.md) | Attach to the SOW |
| 05 | [Technical discovery questionnaire](05-discovery-questionnaire.md) | Send after the prospect agrees to scope a pilot |
| 06 | [Security and data-handling terms](06-security-and-data-handling.md) | Send to the prospect's security reviewer |
| 07 | [Pilot timeline](07-timeline.md) | Plan kickoff and the readout |
| 08 | [Pricing options](08-pricing.md) | Quote |
| 09 | [Acceptance criteria](09-acceptance-criteria.md) | Attach to the SOW; run at the end of the pilot |
| 10 | [Sample report outline](10-sample-report-outline.md) | Show what the customer gets |
| 11 | [Outreach templates](11-outreach-templates.md) | Personalize first-touch and follow-up messages |
| — | [Claims and evidence](CLAIMS.md) | Check a technical statement before you make it |

## Rules for anyone selling this

1. **Claude Code is the only live capture path.** Codex CLI and other agents are
   offered only as a *feasibility replay* of a sample trace, never as production
   capture. Copilot, Cursor, Windsurf, and Gemini CLI have no Korg adapter.
2. **Say "evidence for," never "compliant with."** No certification language, no
   "audit-ready" guarantees, no badges. "Certificate" is the name of Korg's file
   format (`korgcert@v1`), not a compliance certificate. Say so the first time
   you use the word with a prospect.
3. **State the trust limits up front.** An unkeyed local chain shows tampering
   only relative to a tip someone trusts. A sealed Certificate that has left the
   developer's machine fixes the tip from that moment. A rewrite *before* sealing,
   including one by the agent itself, is not detectable. Neither sealing nor public
   anchoring proves a real-world identity or a trusted clock time
   ([CLAIMS.md](CLAIMS.md#limits-to-disclose)).
4. **Ledgers and Certificates contain source code and command output.** Treat
   both as confidential source code, and never publish a Certificate
   ([06](06-security-and-data-handling.md)).
5. **Do not promise unshipped roadmap items** (hosted dashboard, transparency
   log, fully passive capture of every agent, up-to-date crates.io/npm/PyPI
   packages). The crates.io and npm verifier packages are stale 0.1.0 releases;
   never point a prospect at them.
6. **Pin a Korg commit at or after `781bc41`**, never the v0.2.0 tag, which
   predates redaction ([CLAIMS.md](CLAIMS.md)).
7. **Re-check [CLAIMS.md](CLAIMS.md) against `main`** before each quote. The
   product moves quickly; the kit must not get ahead of it.
