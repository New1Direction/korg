# Ideal customer profile

Use this to decide whether a prospect is worth a scoping call, and to disqualify
early when Korg cannot do what they need.

## Primary profile

**An engineering organization of 20–500 engineers that already uses Claude Code
on production repositories and has someone who must answer for what the agents
did.**

| Attribute | Strong fit | Why it matters |
|---|---|---|
| Agent in use | Claude Code, used interactively or headless (`claude -p`), on macOS or Linux | It is the only live capture path ([L1](CLAIMS.md#limits-to-disclose), [L11](CLAIMS.md#limits-to-disclose)) |
| Code host / CI | GitHub and GitHub Actions | The CI gate is a GitHub composite action ([C13](CLAIMS.md#claims-you-may-make)) |
| Audit exposure | SOC 2 or ISO/IEC 27001 in place or in progress; enterprise customers sending security questionnaires; regulated industry (fintech, health tech, insurance, public-sector suppliers) | Someone owns the question "what did the agent change?" |
| Agent maturity | Past the experiment stage: agents edit code that ships, or a wider rollout is planned this quarter | The record matters only once the work is consequential |
| Buyer | A platform/DevEx lead, security engineering lead, or head of GRC with a budget line for AI tooling governance | They can sign a five-figure pilot without a procurement cycle |
| Engineering culture | Comfortable installing open-source CLI tools and reading a hash-chain spec | The pilot runs on their machines, not on our SaaS |

## Buying triggers

Look for one of these in the last 90 days:

- A customer security questionnaire or RFP asked how AI coding assistants are
  governed or logged.
- An audit window (SOC 2 Type II period, ISO surveillance audit) opens within six months
  and AI-assisted change management is a new topic for the auditors.
- An incident or near miss involving an agent: an unexpected file edit, a
  destructive command, a leaked secret in a transcript.
- Claude Code is moving from a few enthusiasts to a team-wide or company-wide
  rollout.
- A public post, talk, or job listing that mentions AI governance, agent
  observability, or "AI SDLC" controls.

## Personas

| Persona | What they care about | Lead with |
|---|---|---|
| **Platform / DevEx lead** | Rolling out agents without slowing developers or creating a security veto | A capture failure never fails a session; the hook's added time is measured in the pilot, not promised; nothing is hosted |
| **Security engineering lead** | Secret leakage, blast radius, "show me what it ran" | Redaction before hashing, canary drill, tamper drills, local-only trust boundary |
| **GRC / compliance owner** | Evidence for change-management and logging controls that an auditor will accept | Independently verifiable records; control-evidence mapping (Evidence Pack); no certification claims |
| **CTO / VP Engineering** | Customer trust, enterprise deals, avoiding a bad headline | A clear yes or no on rollout within a month, at a fixed price |

## Secondary profile (qualify carefully)

- **Teams mainly on OpenAI Codex CLI.** We offer only a feasibility replay of a
  recorded trace ([L1](CLAIMS.md#limits-to-disclose)). Sell Recorder + CI Gate only
  if they also have Claude Code users, or if a feasibility answer alone is worth the
  price to them.
- **Agent-tool vendors and consultancies** that want to attach verifiable
  Certificates to agent-produced deliverables for their own clients. Capture has
  to run on Claude Code.

## Disqualify (do not sell the pilot)

- The team uses only GitHub Copilot, Cursor, Windsurf, Gemini CLI, or another
  agent with no Korg adapter.
- They want a certificate of compliance, an audit opinion, or legal assurance.
- They need a hosted, multi-tenant dashboard, central log collection, SSO, or RBAC
  now. These do not exist ([L8](CLAIMS.md#limits-to-disclose)).
- They need agent actions blocked or approved in real time. This pilot records
  actions; it does not prevent them.
- The pilot developers are on Windows only ([L11](CLAIMS.md#limits-to-disclose)).
- Policy forbids installing Python packages or adding Claude Code hooks on
  workstations, and they will not grant a pilot exception.
- They need a trusted timestamp, or proof that a named person (not a key) produced
  a change ([L3, L4](CLAIMS.md#limits-to-disclose)).

## Qualification checklist (first call)

- [ ] Claude Code in use today on at least one production repository
- [ ] Pilot workstations are macOS or Linux
- [ ] GitHub and GitHub Actions (required for the CI Gate tiers only)
- [ ] A named owner for the agent-governance question
- [ ] A trigger from the list above, with a date
- [ ] Budget authority for the selected tier
- [ ] Agreement that the output is evidence, not certification
