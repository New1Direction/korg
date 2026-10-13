# Technical discovery questionnaire

Send this after the scoping call. Answers decide fit, tier, and pilot plan. We
reply within two business days, either confirming the pilot or explaining why
Korg is not a fit.

Answer only what you can share. Do not include secrets, credentials, source code,
or real session transcripts in your answers.

---

## A. Goals and stakeholders

1. What question should this pilot answer for you? (For example: "Can we show an
   auditor what Claude Code changed?" or "Can we roll Claude Code out to every
   team?")
2. Who owns the pilot decision, and who else must approve rollout (security, GRC,
   platform, legal)?
3. What triggered this now? (Questionnaire, audit date, incident, rollout plan.)
   If there is a date, give it.
4. What happens if the pilot succeeds? What would make you stop it?

## B. Agents in use

5. Which coding agents do your engineers use today, and roughly how many people use
   each? (Claude Code, Codex CLI, Copilot, Cursor, Windsurf, Gemini CLI, internal
   agents, others.)
6. For Claude Code: which version or versions, and how is it run? (Interactive
   terminal, IDE extension, headless `claude -p`, inside CI, in containers or
   remote dev environments.)
7. Do you use Claude Code subagents, custom hooks, MCP servers, or plugins today?
   List any existing hooks so we can avoid conflicts.
8. Do you manage Claude Code settings centrally (managed or enterprise settings)?
   Do they restrict user-defined hooks?
9. For Codex CLI (CI tiers only): can you record one representative WebSocket
   session trace (for example with mitmproxy) and remove authentication headers and
   other secrets before sharing it? We provide a sanitization checklist. If not, we
   skip the feasibility item.

## C. Workstations and environment

10. Pilot developers' operating systems and versions. (macOS and Linux are in
    scope. Windows is not.)
11. Can pilot developers install Python 3.10+ packages from a local checkout of a
    public Git repository (with `--no-deps`), plus `cryptography` from your usual
    index? Is there a proxy or package mirror we must use?
12. Can we build a Rust binary (`korg-verify`) on at least one customer machine or
    CI runner? It is the third independent verifier and lints Certificates for
    secrets directly. Without it, verification uses Python and JavaScript only.
13. Are home directories on network storage or synced (iCloud, OneDrive, roaming
    profiles)? Ledgers are written under `~/.korg`.
14. What endpoint security (EDR, application allow-listing) might flag a new hook
    process?

## D. Repositories and CI

15. Which repositories are in the pilot? Give the approximate size and language, and
    say whether each one is public or private.
16. Code host and CI system. (The CI gate requires GitHub Actions.)
17. Can we add a workflow that runs a composite action from the public Korg
    repository, pinned to a commit? Certificates contain full session content: may
    reviewed Certificates be committed to the pilot repositories, or must CI fetch
    them from a private store? PR comments show the claim, tool counts, and file
    paths, and the action posts them by default. Should we turn them off?
18. Is there a public GitHub repository where you would commit a file containing
    only a tip hash, for public anchoring? (Optional. Only the hash and the commit
    become public, never the Certificate or ledger content.)

## E. Data sensitivity

19. Classification of the code and data that agents touch in the pilot repositories
    (public, internal, confidential, regulated). Do agents ever see production
    data, customer data, or personal data?
20. Ledgers record prompts, commands, file paths, file contents read, and command
    output. Who may hold these records, and where?
21. Which secret formats are common in your environment beyond standard
    provider keys? (Internal token prefixes, connection strings, vault paths.) We
    test redaction coverage against these, using synthetic values only.
22. What may we see during the pilot? Choose one: (a) nothing; we work only on
    screen-share, (b) verifier output and summaries only, (c) selected
    customer-reviewed Certificates, (d) a customer-provisioned sandbox.
23. Do we need to sign a specific NDA, DPA, or security addendum before kickoff?

## F. Audit and governance context

24. Which frameworks or obligations apply? (SOC 2, ISO/IEC 27001, ISO/IEC 42001,
    PCI DSS, HIPAA, NIST AI RMF, EU AI Act, customer contracts, internal policy.)
    When are the next audit dates?
25. Have auditors, customers, or regulators asked about AI-assisted development? If
    you can share the wording of a question, it helps us focus the report.
26. How long must change and logging records be kept under your current policy?
27. Who should hold the issuer signing key after the pilot (security, platform,
    a release engineer)? Do you use an HSM or secrets manager for signing keys today?

## G. Logistics

28. Preferred start date, and any blackout dates (releases, holidays, audit
    fieldwork).
29. Pilot developers: names or roles, time zones, and how many Claude Code working
    sessions each expects to run on pilot repositories during the pilot window.
30. Tier you are considering (Recorder, Recorder + CI Gate, Evidence Pack) and
    anything in [03-scope-and-exclusions.md](03-scope-and-exclusions.md) you would
    like to change.

---

## How we use your answers

| Answer (question number) | Effect |
|---|---|
| No Claude Code use (Q5) | Not a fit for live capture. We say so and stop. |
| Hooks blocked by managed settings, with no exception (Q8) | Not a fit until an exception is granted. |
| Windows-only pilot developers (Q10) | Not a fit. |
| No GitHub Actions (Q16) | Recorder tier only. |
| No public anchoring repository (Q18) | Structural-only anchoring memo replaces the live demonstration. |
| No Rust toolchain anywhere (Q12) | Python and JavaScript are the two independent verifiers; Certificates are linted through their extracted events with the Python lint. |
| Regulated or personal data visible to agents (Q19) | We recommend option (a) or (b) in Q22, and extra canary tests based on Q21. |
| Audit date within 8 weeks (Q24) | We recommend Evidence Pack and an early start, and we flag that the pilot's evidence window will be short. |
