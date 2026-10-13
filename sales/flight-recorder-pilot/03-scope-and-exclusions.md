# Scope and exclusions

Attach this to the statement of work. Tier names, limits, and prices match
[README.md](README.md#pilot-at-a-glance).

## In scope — all tiers

1. **Install capture** for the agreed pilot developers on Claude Code (macOS or
   Linux workstations):
   - Install the `korg-ledger-py`, `claude-code-adapter`, and `korg-seal` Python packages
     from a local checkout of the public Korg repository, at a pinned commit at or
     after `781bc41`. Install them with `--no-deps`, plus `cryptography` from the
     customer's usual index, so that pip never looks up the unregistered
     `korg-ledger-py` name on PyPI ([L10](CLAIMS.md#limits-to-disclose)).
   - Register the `korg-hook` capture hook (`PostToolUse`, `Stop`, `SubagentStop`)
     **per pilot repository**, in that repository's `.claude/settings.local.json`.
     Because the file is created by hand, it is added to `.gitignore` and
     `git check-ignore` confirms it stays out of commits. We also check that
     `~/.claude/settings.json` has no `korg-hook` entry from an earlier
     `korg-setup` run, so only Claude Code sessions in pilot repositories are
     recorded. The hook entries are reviewed with the developer before they are
     written. `korg-setup` is not used ([L16, L17](CLAIMS.md#limits-to-disclose)).
   - Ledgers are written to `~/.korg/sessions/<session_id>.jsonl` on each
     workstation. Custom locations are not used in the pilot, because not every
     Korg command honors `KORG_HOME`.
2. **Backfill (optional).** With the developer's consent, convert existing Claude
   Code session files for the pilot repositories only, with `korg-backfill
   --projects-dir` pointed at each pilot repository's Claude Code project folder.
3. **Verification toolchain.** Set up verifiers from at least two independent
   implementations ([which verifier checks what](CLAIMS.md#which-verifier-checks-what)):
   - Python: `python -m korg_ledger lint` (ledgers) and `korg-seal verify`
     (Certificates).
   - JavaScript: `verify.mjs` from Node.js, or the browser pages for reviewers.
   - Rust: `korg-verify`, built from source on at least one customer machine or CI
     runner where a Rust toolchain is allowed. It lints a Certificate for secrets
     directly (`--require-redacted`). Without Rust, the Python lint checks a
     Certificate's extracted events instead
     ([which verifier checks what](CLAIMS.md#which-verifier-checks-what)).
   - Do not use the `@korgg/ledger-verify` npm package or the `korg-verify` crate
     from crates.io. Both are stale 0.1.0 releases that predate Certificates and
     redaction ([L10](CLAIMS.md#limits-to-disclose)).
4. **Issuer key setup.** The customer generates an Ed25519 issuer key with
   `korg-seal` and records its public key as the pinned key. The customer also
   chooses a sealing model ([06 §5](06-security-and-data-handling.md#5-keys)).
5. **Certificates.** Mint `korgcert@v1` Certificates for a sample of pilot
   sessions chosen by the customer, only from ledgers that pass the redaction lint.
6. **Drills.** Run the tamper, redaction-canary, and wrong-key drills defined in
   [09-acceptance-criteria.md](09-acceptance-criteria.md).
7. **Measurement.** Capture coverage (transcript tool calls compared with ledger
   events), hook overhead, and the time between capture and sealing, on real pilot
   sessions.
8. **Findings report and readout** following
   [10-sample-report-outline.md](10-sample-report-outline.md).

## Additional scope — Recorder + CI Gate

9. **CI verification gate.** Add a workflow to the pilot repositories that runs the
   `verify-korgcert` GitHub composite action, referenced by commit SHA and pinned
   to the customer's issuer key, so that tampered or unverifiable Certificates
   fail the job. Because the action does not check redaction, the workflow also runs
   a redaction-lint step: `korg-verify --require-redacted` built in the job, or the
   Python lint over each Certificate's extracted events. Because Certificates
   contain full session content, the customer chooses where the CI job reads them
   from: committed to the pilot repository after review, or fetched from a private
   customer store. PR comments (which show the claim, tool counts, and file paths)
   are on by default in the action, so the workflow sets `comment: "false"` unless
   the customer turns them on.
10. **Public anchoring (opt-in).** If the customer agrees, commit a file that
    contains **only** a Certificate's tip hash to a public GitHub repository, bind
    it with `korg-seal anchor`, and show `korg-seal resolve`. The Certificate itself
    is never published ([L15](CLAIMS.md#limits-to-disclose)). If the customer does
    not opt in, deliver a memo that explains the structural-only option and what
    it does not prove.
11. **Second-agent feasibility.** The customer records and sanitizes one
    OpenAI Codex CLI WebSocket trace, following our sanitization checklist (the
    capture includes authentication headers). We write a short replay script,
    which the customer keeps, to run the trace through the `codex-ws` adapter into a
    `korg-ledger@v1` journal. We verify that journal and report what production
    capture of Codex would take. This is a feasibility finding, not a capture
    deployment.

## Additional scope — Evidence Pack

12. **Control-evidence mapping workshop** (half day, remote). With the customer's
    GRC owner, map pilot artifacts to the controls *the customer selects* from its
    own framework, for example change-management and logging controls in SOC 2 or
    ISO/IEC 27001. The output is a draft evidence matrix for the customer and its
    auditor to review. It is not an audit opinion.
13. **Operating runbook.** Written procedures for rollout beyond the pilot group,
    key custody and rotation, ledger retention, redaction incidents (using
    `rechain-redacted`), and verifier upgrades.
14. **Executive readout** for a second audience, such as leadership or the audit
    committee.

## Exclusions — all tiers

The following are out of scope. Any of them can be quoted separately through a
change order ([08-pricing.md](08-pricing.md#change-orders)).

- **Certification or attestation.** No compliance certification, audit opinion,
  legal advice, or statement that the customer meets any standard or regulation.
- **Agents with no live capture path.** Live capture of GitHub Copilot, Cursor,
  Windsurf, Gemini CLI, Codex CLI, or any agent other than Claude Code. Codex
  appears only as the feasibility replay in item 11.
- **Hosted or networked components.** No hosted service, central log server, or
  shared dashboard. `korg-server` and `mcp_server.py` are not deployed on any
  network ([L8](CLAIMS.md#limits-to-disclose)).
- **The `stream-json` adapter**, because its blob store is not redacted
  ([L7](CLAIMS.md#limits-to-disclose)).
- **Prevention.** No blocking, approving, or sandboxing of agent actions.
- **Fleet rollout.** Endpoint-management packaging (MDM, Ansible, golden images)
  and installs beyond the tier's developer limit.
- **Windows workstations** ([L11](CLAIMS.md#limits-to-disclose)).
- **DLP and data classification.** Redaction beyond `korg-redact@v1` rules; detection
  of personal data or proprietary code inside ledgers.
- **Trusted timestamping, identity binding, and key registries** beyond the
  pinned-key and git-tip mechanisms in [CLAIMS.md](CLAIMS.md).
- **Changes to Korg itself.** New adapters, features, or bug fixes. Bugs found
  during the pilot are reported upstream through the public repository (security
  issues through [SECURITY.md](../../SECURITY.md)). Fixing them is not a pilot
  deliverable.
- **Code quality review** of agent-produced changes. A Certificate shows what
  happened, not whether the work was good ([L5](CLAIMS.md#limits-to-disclose)).
- **Onsite work and travel**, unless agreed in writing and billed at cost.

## Customer responsibilities

- Name a pilot owner who can make decisions and attend the weekly check-in.
- Provide pilot developers who use Claude Code on the pilot repositories during the
  pilot window (at least three working sessions each).
- Provide approvals before kickoff: workstation install exception, hook
  registration, and (for CI tiers) permission to add a GitHub Action.
- Hold the issuer private key. We never take custody of it.
- Review and approve every artifact before it is shared with us
  ([06-security-and-data-handling.md](06-security-and-data-handling.md)).
- Return the [discovery questionnaire](05-discovery-questionnaire.md) before kickoff.

Delays caused by missing access or approvals move the timeline without changing
the price. Longer pauses are handled under
[08-pricing.md](08-pricing.md#payment-terms).
