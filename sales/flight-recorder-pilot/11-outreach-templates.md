# Outreach templates

Templates for first-touch and follow-up messages to teams using coding agents.
**Nothing in this kit has been sent to anyone.** A person sends each message
individually, after doing the research below.

## Before you write

Every message must include at least one specific, verifiable fact about the
prospect. If you cannot find one, do not send.

| Token | What to put there | Where to find it |
|---|---|---|
| `{{FIRST_NAME}}` | Recipient's first name | Profile, company site |
| `{{COMPANY}}` | Company name | — |
| `{{AGENT_SIGNAL}}` | Evidence that they use Claude Code: a blog post, talk, job listing, `CLAUDE.md` in a public repo, a public Claude Code hook or config | Engineering blog, GitHub, job boards, conference talks |
| `{{AUDIT_SIGNAL}}` | Evidence of audit exposure: trust page listing SOC 2 / ISO 27001, a regulated industry, enterprise customers | Trust center, security page, customer logos |
| `{{TRIGGER}}` | A recent, dated reason this matters now: rollout announcement, AI-governance hire, published AI policy, incident write-up | News, blog, job boards |
| `{{SELLER_NAME}}`, `{{SELLER_EMAIL}}` | You | — |

Rules:

- **Claude Code users only** for the live-capture pitch. If the only signal is
  Copilot, Cursor, or another agent with no adapter, do not send
  ([02](02-ideal-customer-profile.md#disqualify-do-not-sell-the-pilot)).
- **No compliance claims.** Never write "compliant," "certified," "audit-ready," or
  "satisfies <framework>." Write "evidence your auditors can check."
- **No invented numbers** about their team, and no performance claims.
- **One ask per message**: a 30-minute call.
- Follow applicable law and the recipient's stated contact preferences (for
  example CAN-SPAM, GDPR/PECR, CASL). Include an opt-out line in cold email. Stop
  after the second follow-up.
- Links to include, all public: the [30-second tamper demo](https://github.com/New1Direction/korg#30-second-quickstart),
  the [browser Certificate verifier](https://new1direction.github.io/korg/web/seal.html),
  and the [one-page offer](01-offer.md), sent as an attachment or PDF.

---

## 1. Cold email — platform or DevEx lead

**Subject:** What Claude Code did in {{COMPANY}}'s repos, verifiably

Hi {{FIRST_NAME}},

I saw {{AGENT_SIGNAL}}. When Claude Code edits code that ships, the only record of
what it did is the transcript on that developer's laptop, and nobody downstream
can check it.

We run a fixed-price, two-to-four-week pilot that installs Korg, an open-source
flight recorder for Claude Code. Each session becomes a hash-chained record of
prompts, tool calls, and results. Recognized secrets are redacted before anything
is written. Selected sessions become signed Certificates that a reviewer can
verify offline, in CI or in a browser tab that uploads nothing. Everything runs
on your machines, and nothing is hosted by us.

Here is a 30-second demo of a one-character edit being caught:
https://github.com/New1Direction/korg#30-second-quickstart

Would a 30-minute call to see whether it fits {{COMPANY}}'s setup be useful?

{{SELLER_NAME}}

*If this isn't relevant, reply "no" and I won't follow up.*

---

## 2. Cold email — security engineering lead

**Subject:** Agent tool calls at {{COMPANY}}: can you show what ran?

Hi {{FIRST_NAME}},

{{AGENT_SIGNAL}} suggests Claude Code is running shell commands and editing code
at {{COMPANY}}. If one of those sessions leaked a credential or ran something
destructive, could you show exactly what happened, in a form someone else can
verify?

Korg is an open-source recorder that does this for Claude Code:

- Every tool call and result goes into a SHA-256 hash chain. Once a session is
  sealed into a signed Certificate, edit, delete, or reorder one event and
  independent verifiers (Rust, Python, JavaScript) name the exact break.
- Recognized secrets (provider keys, bearer tokens, JWTs, private keys, credentialed
  URLs) are replaced before hashing. In the pilot we test this with planted canaries
  in your own formats and report what is not covered.
- It is local only, with no hosted collector. You hold the signing key.

We sell it as a fixed-price pilot with written acceptance criteria. We tell you
the limits before you sign (for example, live capture is Claude Code only, and a
signature proves a key, not a person).

Open to a 30-minute call?

{{SELLER_NAME}}

*Not relevant? Reply "no" and I'll stop here.*

---

## 3. Cold email — GRC or compliance owner

**Subject:** Evidence for AI-assisted changes ahead of {{COMPANY}}'s next audit

Hi {{FIRST_NAME}},

{{AUDIT_SIGNAL}}, and {{AGENT_SIGNAL}}. When auditors or enterprise customers ask
how AI-assisted code changes are logged and reviewed, the honest answer today
is usually "we trust the developer."

We run a short, fixed-price pilot that gives your engineering team a verifiable
record of what Claude Code did. The records are hash-chained, have secrets
redacted, and are signed with a key you hold. Your reviewers can check them
without trusting us or the tool. In the Evidence Pack option, we work with you to
draft a matrix linking those records to the controls *you* select, for your
auditor to evaluate.

To be clear, this is not a certification or an audit opinion. It produces evidence,
and it is open about what that evidence does and does not prove.

Worth 30 minutes before {{TRIGGER}}?

{{SELLER_NAME}}

*If this isn't for you, reply "no" and I won't write again.*

---

## 4. Cold email — team on Codex CLI and Claude Code

Use only when there is evidence of **both** agents.

**Subject:** One verifiable record format for Claude Code and Codex at {{COMPANY}}

Hi {{FIRST_NAME}},

It looks like {{COMPANY}} uses both Claude Code and Codex CLI ({{AGENT_SIGNAL}}).
Korg records Claude Code sessions as tamper-evident, independently verifiable
ledgers today. Codex is earlier: Korg can translate a recorded Codex trace into the
same format, but it does not capture Codex live yet.

Our pilot installs live capture for your Claude Code users and replays one of your
Codex traces, so you get a straight answer on both before you commit to
anything. It is fixed price, and it runs on your machines.

30 minutes to see if it fits?

{{SELLER_NAME}}

*Reply "no" and I'll stop here.*

---

## 5. LinkedIn connection note (≤ 300 characters)

> Hi {{FIRST_NAME}}, I saw {{AGENT_SIGNAL}}. I help teams on Claude Code keep a
> tamper-evident, independently verifiable record of what the agent ran and
> changed (open source, runs locally). Happy to share the 30-second demo if useful.

---

## 6. Follow-up 1 (3–4 business days later)

**Subject:** Re: {{ORIGINAL_SUBJECT}}

Hi {{FIRST_NAME}},

A quick way to judge whether this is real: open
https://new1direction.github.io/korg/web/seal.html and click "a real seal", then
"forged summary". The page re-derives the summary from the events and catches the
forgery. You can also edit any character of the Certificate in the text box and
verify again. It all runs in your browser.

If a call isn't useful right now, no reply is needed.

{{SELLER_NAME}}

---

## 7. Follow-up 2 — last message (7 business days after follow-up 1)

**Subject:** Closing the loop

Hi {{FIRST_NAME}},

I won't keep writing. If agent audit trails come up later, for example in a
security questionnaire or a wider Claude Code rollout, the one-page offer is
attached and I'm at {{SELLER_EMAIL}}.

{{SELLER_NAME}}

---

## 8. Post-call recap

**Subject:** {{COMPANY}} × Korg pilot: recap and next step

Hi {{FIRST_NAME}},

Thanks for the time today. What I heard:

- Goal: {{GOAL_IN_THEIR_WORDS}}
- Agents and setup: {{AGENTS_AND_SETUP}}
- Timing driver: {{TRIGGER}}
- Fit concerns we discussed: {{CONCERNS}}

Suggested tier: **{{TIER}}** ({{PRICE}}, {{DURATION}}). Scope, exclusions, and
acceptance criteria are attached, along with our security and data-handling terms
for your security reviewer.

Next step: the attached technical discovery questionnaire. Within two business
days of your answers we will confirm the pilot or tell you plainly that Korg is
not a fit, before any invoice.

{{SELLER_NAME}}

---

## Objection replies

**"Are you SOC 2 certified?" / "Will this make us compliant?"**
> No, and we won't claim that. Korg is open-source tooling, and the pilot produces
> evidence (verifiable records of agent activity) that you and your auditors can
> evaluate. Whether it meets a given control is your auditors' call. In Evidence
> Pack we help you draft that mapping for them to review.

**"We already have logs."**
> Logs are useful, but whoever controls a log can edit it, and a reviewer has to
> trust it. Korg's records are hash-chained, and once a session is sealed into a
> signed Certificate that leaves the developer's machine, any later edit is caught
> by verifiers you run yourself. The Certificate also binds the summary a reviewer
> reads to the underlying events.

**"Can't the developer (or the agent) just rewrite the whole chain?"**
> Before the session is sealed, yes. The agent runs as the developer's user, so it
> could too. A local unkeyed chain is tamper-evident only
> relative to a tip someone else holds. Sealing a session into a Certificate fixes
> its tip. Once that Certificate has left the machine (committed for CI, stored
> centrally, or anchored by its hash to a public commit), any later rewrite is
> caught. The
> window between capture and sealing is real, so the pilot measures it and the
> runbook decides who seals and when. It's spelled out in our security terms.

**"We use Cursor / Copilot."**
> Korg doesn't capture those today, so the pilot isn't a fit, and we'd rather tell
> you now. If Claude Code is in your plans, we're happy to talk then.

**"Why pay for something open source?"**
> You're paying for a working installation on your machines, drills run against
> your own sessions, measured coverage and overhead, and a report your security and
> GRC owners can act on, delivered in a fixed time with written acceptance criteria.
> You keep everything, with no license fee.
