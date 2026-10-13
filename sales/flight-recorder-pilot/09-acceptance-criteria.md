# Acceptance criteria

The pilot is accepted when every criterion for the selected tier passes. Each
criterion is checked with commands the customer runs, or watches run, on its own
machines. The "Known limitations" below were disclosed before signature. They are
not criteria, and they cannot be used to waive one.

"Two independent verifiers" means verifiers from two different implementations:
Rust, Python, or JavaScript. The Node.js CLI and the browser pages run the same
JavaScript code, so they count as one. The commands for each check are in
[which verifier checks what](CLAIMS.md#which-verifier-checks-what):

- **Ledgers:** `korg-verify`, `python -m korg_ledger lint`, or `verify.mjs`
  (Node or browser).
- **Certificates:** `korg-verify --pubkey`, `korg-seal verify --pin`, or
  `verify.mjs --pubkey` (Node or browser). `korg_ledger lint` cannot read
  Certificates.

## AC1 — Capture works

- The capture hook is registered in each pilot repository for every pilot
  developer, at the commit SHA recorded in the runbook (D2).
- At least **3 working sessions per pilot developer** on pilot repositories each
  produce a per-session ledger in `~/.korg/sessions/`.
- A Claude Code session started outside the pilot repositories produces no ledger.
- No pilot developer reports a Claude Code session failing or blocking because of
  the hook. Any slowdown is recorded under AC3.
- Uninstalling with the runbook removes the hook entry and leaves the ledgers in place.

## AC2 — Ledgers verify independently

- **Every pilot ledger** verifies as VALID (chain intact, causal DAG well-formed)
  under **two independent verifiers**, and the two agree. Redaction findings from
  `korg_ledger lint` are judged under AC6, not here.
- Any ledger that fails is investigated and its root cause is written up. If the
  cause is a Korg defect, we report it upstream. The criterion still fails, unless
  the customer accepts in writing with the defect listed as a finding.

## AC3 — Coverage is measured and explained

- For at least **10 sampled sessions** (or every session, if there are fewer than
  10), the report compares the number of `tool_use` blocks in the Claude Code
  transcript with the tool-call events in the ledger, per tool name.
- Every difference is explained in the report (for example, a tool call with no
  result because the session ended). Unexplained differences fail this criterion.
- Hook overhead (time per firing compared with transcript size) is measured on
  real pilot sessions and reported with the Claude Code version. **No overhead
  threshold is promised.** The number is a finding for the rollout decision.

## AC4 — Tampering is detected

On copies of real pilot ledgers, in front of the customer:

| Drill | Expected result |
|---|---|
| Change one character in one event's content | Every verifier reports INVALID, names the edited `seq`, and exits non-zero |
| Delete one event from the middle | INVALID, localized to the break |
| Swap the order of two events | INVALID |
| Edit a Certificate's summary (for example, remove a file from `files`) | Certificate INVALID (summary does not match the events) |
| Verify a valid Certificate with a different pinned key | INVALID (issuer does not match pinned key) |

All five drills produce the expected result in two independent verifiers.

## AC5 — Certificates verify under the pinned key

- The customer generates the issuer key on a customer machine. The private seed is
  never transferred to the vendor.
- At least **3 Certificates** are minted from real pilot sessions chosen by the
  customer.
- Each Certificate verifies as VALID with the customer's public key pinned, in two
  independent verifiers. Reviewers are also shown the browser page.
- The developer reviews each Certificate summary (agents, tool counts, file paths
  from file-tool arguments, seq range) against their memory of the session. Any
  mismatch is a finding. Files changed only through shell commands are expected to
  be missing from `files` ([L14](CLAIMS.md#limits-to-disclose)).

## AC6 — Secrets are redacted before hashing

- Synthetic canary secrets, one per format from
  [questionnaire item 21](05-discovery-questionnaire.md#e-data-sensitivity) plus at
  least one standard provider key and one bearer token, are passed through a real
  Claude Code session (for example, echoed in a shell command).
- For every format covered by `korg-redact@v1`, the ledger contains the
  `[REDACTED:<rule>]` marker and **does not contain the raw canary value**, and the
  ledger still verifies.
- Formats that are not covered are listed in the report with a mitigation. They
  are a finding, not a failure, because coverage of customer-specific formats is a
  [disclosed limit](CLAIMS.md#limits-to-disclose).
- Every pilot ledger passes the redaction lint (`python -m korg_ledger lint` or
  `korg-verify --require-redacted`), and every Certificate is minted from a ledger
  that passed. Every Certificate also passes the lint: `korg-verify
  --require-redacted`, or the Python lint over its extracted events
  ([how](CLAIMS.md#which-verifier-checks-what)). A ledger that fails is remediated with
  `rechain-redacted`, and the exposed secret is rotated
  ([06 §4](06-security-and-data-handling.md#4-secrets-and-redaction)).

## AC7 — CI gate passes good and fails bad (CI tiers)

- The CI gate runs on the pilot repositories: the `verify-korgcert` action,
  referenced by commit SHA and pinned to the customer's issuer key, plus a
  redaction-lint step.
- A PR with a valid Certificate passes the job.
- A PR with a deliberately tampered Certificate fails the job, and the job output
  names the problem.
- A PR with a Certificate signed by a different key fails the job.

## AC8 — Report and readout delivered

- The findings report follows [10-sample-report-outline.md](10-sample-report-outline.md)
  and includes every tier deliverable (D9, plus D11–D12 for CI tiers and D13–D15
  for Evidence Pack).
- The readout is held with the pilot owner. For Evidence Pack, the executive
  readout is held too.
- The report's appendix lists the exact commit SHA, package versions, Claude Code
  version, and the commands used, so the customer can reproduce every result.

## Known limitations (not criteria)

These are disclosed before signature ([CLAIMS.md](CLAIMS.md#limits-to-disclose)).
The pilot does not test for them, so they cannot fail it, and they cannot excuse a
failed criterion either:

- No live capture of agents other than Claude Code.
- A ledger can be rewritten undetectably before it is sealed, including by the
  agent itself.
- No trusted timestamp; public anchoring requires a public GitHub repository.
- Signatures prove a key, not a person, and there is no revocation.
- Redaction does not cover formats outside `korg-redact@v1`.
- Tool results longer than 8,000 characters are truncated, and the summary's
  `files` list covers file-tool arguments only.
- Actions outside Claude Code (for example, manual edits by the developer) are not
  recorded.

## Acceptance process

1. The vendor runs the acceptance checks with the customer and sends the results
   with the report.
2. The customer has **5 business days** to accept or to list, in writing, the
   criteria it believes failed.
3. The vendor fixes listed failures within **5 business days** and re-runs only
   those criteria.
4. If the customer neither accepts nor lists failures within 5 business days of
   receiving the report, the pilot is deemed accepted.
5. Criteria blocked by missing customer access or approvals are rescheduled, not
   failed ([03](03-scope-and-exclusions.md#customer-responsibilities)).
