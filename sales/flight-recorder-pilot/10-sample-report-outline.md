# Sample findings report outline

This is the structure of the report delivered as D9. Example values are shown in
*italics* to show the level of detail; they are illustrative, not results from a
real customer. Sections marked **CI** or **EP** appear only in those tiers.

---

## Agent Flight-Recorder Pilot — Findings Report

**Customer:** {{CUSTOMER}} · **Tier:** {{TIER}} · **Pilot window:** {{START}}–{{END}}
**Korg commit:** {{KORG_SHA}} · **Claude Code version(s):** {{CC_VERSIONS}}
**Prepared by:** {{SELLER_NAME}} · **Classification:** Customer Confidential

> This report presents evidence gathered during a technical pilot. It is not a
> certification, attestation, audit opinion, or legal advice.

### 1. Executive summary (one page)

- The question the pilot set out to answer, in the customer's words.
- **Recommendation:** roll out / roll out with conditions / do not roll out, with
  the two or three reasons that decide it.
- Headline numbers: *sessions captured: 41 across 5 developers · ledgers verified:
  41/41 in two verifiers · tool-call coverage in sampled sessions: 612/615, with all
  3 gaps explained · canary formats redacted: 7/9 · median hook time per firing:
  measured value*.
- Acceptance criteria status (AC1–AC8): one line each.

### 2. Scope and environment

- Repositories, pilot developers (roles, not names, unless the customer prefers
  names), operating systems, Claude Code versions and run modes.
- What was installed, where, and at which commit SHA.
- Data-sharing level used ([06](06-security-and-data-handling.md#3-vendor-access)).
- Deviations from the SOW and the reasons for them.

### 3. How the record works (one page, for non-engineers)

- Capture → redaction → hash chain → Certificate → independent verification, as
  a simple diagram.
- What a green verification proves, and what it does not
  ([CLAIMS.md](CLAIMS.md#limits-to-disclose)), in plain language.

### 4. Capture results (AC1, AC3)

- Sessions captured per developer and per repository.
- Coverage table for sampled sessions:

  | Session | Transcript tool calls | Ledger tool events | Difference | Explanation |
  |---|---|---|---|---|
  | *s-0193…* | *48* | *48* | *0* | — |
  | *s-01a2…* | *77* | *76* | *1* | *Session ended before the last `Bash` result was written* |

- Scope check: no ledgers from sessions outside the pilot repositories.
- Subagent sessions: how they appear (sidechain events are flattened into the
  parent session).
- Backfill results, if used.

### 5. Integrity verification (AC2)

- Verdicts per verifier implementation, and their agreement.
- Any failing ledger, with its root cause.

### 6. Tamper drills (AC4)

- One row per drill: what was changed, the expected result, each verifier's actual
  output (verbatim), and pass or fail.

### 7. Redaction and secrets (AC6)

- Canary formats tested; redacted or not; the rule that matched.
- Lint results across all pilot ledgers.
- Uncovered formats and recommended mitigations (for example, rename internal
  token variables to match a sensitive-key rule, or keep those secrets out of agent
  sessions).
- Any real secret found in a ledger, how it was remediated, and confirmation of
  rotation (identifiers only, never values).

### 8. Certificates and keys (AC5)

- Certificates minted: session, claim, summary, verification output under the
  pinned key.
- Sealing model chosen ([06 §5](06-security-and-data-handling.md#5-keys)), the
  measured time between capture and sealing, and recommendations for key custody.

### 9. CI verification gate — **CI** (AC7)

- Workflow configuration: action referenced by SHA, pinned key, redaction-lint
  step, where Certificates are read from, and whether PR comments are on.
- Links to the passing and deliberately failing PR runs, with job output.
- A recommended policy for which PRs should carry a Certificate.

### 10. Public anchoring — **CI**

- Anchored Certificate and `korg-seal resolve` output, or the structural-only memo
  explaining what the customer gives up without a public anchor.

### 11. Second-agent feasibility (Codex CLI) — **CI**

- What was replayed, verification result, causal-structure observations.
- What production capture would require, and an honest estimate of effort.

### 12. Performance and developer experience

- Hook time per firing compared with transcript size, measured on real sessions.
  Include the method and the machine types.
- Developer feedback: any perceived slowdown, confusion, or friction.
- Disk use of `~/.korg` per developer per week.

### 13. Gaps, risks, and limits

- Product limits that affect this customer, cross-referenced to
  [CLAIMS.md](CLAIMS.md#limits-to-disclose).
- Defects found and reported upstream (public issue links; security issues
  referenced only by private advisory).
- Operational risks of a rollout (for example key custody, ledger retention on
  laptops, Claude Code upgrades changing the transcript format).

### 14. Control-evidence mapping — **EP**

- Draft matrix. Columns: customer-selected control (ID and text, as the customer
  chose them), the evidence artifact, how a reviewer verifies it, its limits, and
  the owner. Example rows the customer might select: *SOC 2 CC8.1 (change
  management)*, *ISO/IEC 27001:2022 Annex A 8.15 (logging) and 8.32 (change
  management)*.
- A statement that the matrix is a draft for the customer's auditor to evaluate.

### 15. Recommendations and 90-day plan

- Rollout decision and conditions.
- Concrete next steps with owners: wider capture group, CI policy, key custody,
  retention, redaction hardening, monitoring of upstream Korg releases.
- What to re-measure after 90 days.

### Appendices

- A. Exact commands used for every acceptance check, so the results can be
  reproduced.
- B. Versions: Korg commit SHA, Python package versions, Node.js version, Claude Code
  versions, operating systems.
- C. SHA-256 of every artifact referenced in the report (ledgers, Certificates).
- D. Verbatim verifier output for drills and Certificates.
- E. Glossary: ledger, hash chain, tip, causal DAG, Certificate, issuer key,
  pinning, anchor, redaction marker.
- F. Operating runbook (**EP**), or a link to it.
