# Deliverables

Every deliverable stays in the customer's environment and belongs to the customer.
Items marked **CI** are in Recorder + CI Gate and Evidence Pack. Items marked
**EP** are in Evidence Pack only.

| # | Deliverable | Format | Tier | Accepted by |
|---|---|---|---|---|
| D1 | **Working capture** in each pilot repository on each pilot developer's workstation, at a pinned Korg commit at or after `781bc41` | Installed hook + per-session ledgers in `~/.korg/sessions/` | All | [AC1](09-acceptance-criteria.md#ac1--capture-works) |
| D2 | **Install and uninstall runbook** for the pilot configuration, including exact hook entries and the rollback step | Markdown | All | [AC1](09-acceptance-criteria.md#ac1--capture-works) |
| D3 | **Verification kit**: verifiers from at least two independent implementations (including Rust `korg-verify` where a Rust toolchain is allowed), plus a one-page "how to verify a session or Certificate" guide for reviewers | Installed tools + Markdown | All | [AC2](09-acceptance-criteria.md#ac2--ledgers-verify-independently) |
| D4 | **Coverage measurement** comparing Claude Code transcripts with ledger events for sampled sessions | Table in the report + script output | All | [AC3](09-acceptance-criteria.md#ac3--coverage-is-measured-and-explained) |
| D5 | **Customer-held issuer key**, its pinned public key, and the chosen sealing model, recorded in the runbook | Key file on a customer machine (never sent to us) | All | [AC5](09-acceptance-criteria.md#ac5--certificates-verify-under-the-pinned-key) |
| D6 | **Sample Certificates** (`korgcert@v1`) for customer-chosen sessions | `.korgcert.json` files | All | [AC5](09-acceptance-criteria.md#ac5--certificates-verify-under-the-pinned-key) |
| D7 | **Drill record**: tamper, deletion, wrong-key, and redaction-canary drills with verifier output | Report appendix with command logs | All | [AC4](09-acceptance-criteria.md#ac4--tampering-is-detected), [AC6](09-acceptance-criteria.md#ac6--secrets-are-redacted-before-hashing) |
| D8 | **Overhead measurement**: hook time per firing against session length on real pilot sessions | Table in the report | All | [AC3](09-acceptance-criteria.md#ac3--coverage-is-measured-and-explained) |
| D9 | **Findings report** and a 60-minute **readout** | PDF or Markdown + meeting | All | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |
| D10 | **CI verification gate** on the pilot repositories (verification pinned to the issuer key, plus a redaction-lint step), with a passing PR and a deliberately failing PR as proof | Workflow file + two PR runs | CI | [AC7](09-acceptance-criteria.md#ac7--ci-gate-passes-good-and-fails-bad-ci-tiers) |
| D11 | **Anchoring demonstration** (if the customer opts in to publishing a tip hash, never the Certificate) or a written structural-only alternative | Anchored Certificate + `resolve` output, or memo | CI | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |
| D12 | **Codex CLI feasibility finding** from one replayed, customer-sanitized trace | Replay script + verified journal + one-page memo | CI | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |
| D13 | **Draft control-evidence matrix** linking pilot artifacts to customer-selected controls, for the customer's auditor to review | Spreadsheet or Markdown table | EP | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |
| D14 | **Operating runbook**: rollout, key custody and rotation, retention, redaction incident response, verifier upgrades | Markdown | EP | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |
| D15 | **Executive readout** for a second audience | Slides + meeting | EP | [AC8](09-acceptance-criteria.md#ac8--report-and-readout-delivered) |

## What the customer does not get

- A certificate of compliance, audit opinion, or legal assessment
  ([03 exclusions](03-scope-and-exclusions.md#exclusions--all-tiers)).
- A hosted service, account, or subscription. Nothing needs to be renewed for the
  installed tooling to keep working.
- Proprietary software. Everything installed is the public, MIT / Apache-2.0
  Korg code at the pinned commit. Pilot scripts written for the customer are
  delivered under the same licenses.
