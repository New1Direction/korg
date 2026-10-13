# Agent Flight-Recorder Pilot

**A fixed-price pilot that gives your team a verifiable record of what its coding
agents actually did. Once a session is sealed, your security reviewers and
auditors can check its record themselves, without trusting us or the tool that
produced it.**

## The problem

Your engineers already let Claude Code read files, run shell commands, and edit
code in real repositories. When someone asks "what did the agent do on that
change?", the answer today is a chat transcript or a log file on a laptop. Anyone
with access could have edited it, and nobody downstream can check it.

Security questionnaires, enterprise customers, and internal risk owners are
starting to ask that question. "We trust the developer" is a weak answer.

## What the pilot does

In two to four weeks, on your machines and your repositories, we:

1. **Record.** We install Korg's open-source capture hook for Claude Code in the
   pilot repositories of a small group of developers. Every session becomes its
   own ledger: prompts, model turns, tool calls, and tool results (long results are
   truncated at 8,000 characters), causally linked and SHA-256 hash-chained.
2. **Redact before anything is written.** Recognized secrets (API keys, bearer
   tokens, private keys, JWTs, credentialed URLs) are replaced before an event is
   hashed or saved, and we prove it with a planted canary secret.
3. **Seal selected sessions.** For the changes that matter, we mint a signed
   Certificate (`korgcert@v1`, Korg's file format, not a compliance certificate).
   It embeds the session's events plus a summary of agents, tool counts, and file
   paths used by file tools. The summary is re-derived from the events, so it
   cannot disagree with the record.
   Anyone can verify it offline with a command-line verifier (Rust, Python, or
   Node.js) or a browser tab that uploads nothing.
4. **Prove tampering is caught.** We edit records, delete them, and re-sign them with
   the wrong key in front of your team and show each verifier name the exact break.
5. **Gate CI** (Recorder + CI Gate and Evidence Pack). A GitHub Action verifies
   Certificates against your pinned key on pull requests and fails the job on any
   tampered or unverifiable file.
6. **Report.** You get a written findings report with coverage numbers, drill
   results, measured overhead, gaps, and a recommendation on whether to roll out.

## What you can do afterwards

- Answer "what did the agent do in the session behind PR #1234?" with a record a
  third party can verify, not a screenshot.
- Give security and GRC owners artifacts they can inspect themselves when they
  assess change-management and logging controls for AI-assisted development.
- Decide on a rollout using your own measurements of coverage and overhead,
  not vendor claims.

## Why this is credible

- **Open source, no lock-in.** Korg is MIT / Apache-2.0. The format
  (`korg-ledger@v1`) is a public spec with three independent verifiers (Rust,
  Python, JavaScript) that pass the same conformance vectors. You keep everything
  we install.
- **It runs in your environment.** Capture, sealing, and verification all happen on
  your machines. We run no hosted service, and nothing is sent to us unless you
  choose to share it.
- **We disclose the limits.** Live capture is Claude Code only. A signature proves
  a key, not a person. Until a session is sealed, its record can be rewritten by
  whoever controls the machine, including the agent itself. A sealed Certificate
  fixes it from that point on, and the pilot sets out who seals and when. The full
  list is in [CLAIMS.md](CLAIMS.md#limits-to-disclose).

## What this is not

This is not a compliance certification, an audit, or legal advice. The pilot
produces evidence that you and your auditors may use. Whether that evidence meets
any control or regulatory requirement is your auditors' call.

## Options

| | Recorder | Recorder + CI Gate | Evidence Pack |
|---|---|---|---|
| Price | $6,500 | $14,500 | $24,000 |
| Duration | 2 weeks | 3 weeks | 4 weeks |
| Repositories / pilot developers | 1 / up to 5 | up to 2 / up to 10 | up to 3 / up to 20 |

Fixed price, 50% at signature and 50% on acceptance against
[written criteria](09-acceptance-criteria.md). Full comparison:
[08-pricing.md](08-pricing.md).

## Next step

A 30-minute scoping call, then our [technical discovery questionnaire](05-discovery-questionnaire.md).
We confirm fit within two business days of receiving your answers. If Korg cannot
capture your agents, we will tell you then, before you pay anything.

Contact: {{SELLER_NAME}} · {{SELLER_EMAIL}}
