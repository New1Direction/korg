# The Gold Seal — the trust layer for AI-agent work

> **One line:** AI agents now do real work — write code, move money, file tickets,
> touch production. Nobody can independently verify *what an agent actually did*.
> The **Gold Seal** is a portable, cryptographically-signed certificate of an agent
> session that **anyone can re-verify offline, with zero trust in the issuer**.

This document is positioning, not a spec. Every capability it claims is backed by
working, cross-language-conformant code — see [`GOLDSEAL.md`](../../spec/korg-ledger-v1/GOLDSEAL.md)
for the normative spec and the "what we do NOT claim" section at the end.

---

## Why now

Three things became true in the same 18 months:

1. **Agents act, not just chat.** Coding agents open PRs; ops agents run commands;
   finance agents move money. The output is *consequential*.
2. **The output is unattributable.** A diff, a deploy, a report — none of it carries
   proof of *which agent did it, from what prompts, touching what, in what order*. A
   screenshot of a chat is not evidence. A log the producer controls is not evidence.
3. **Someone is about to be on the hook.** Enterprises adopting agents need an audit
   trail. Regulators are circling "AI provenance." Marketplaces of agent work need a
   way to tell real deliverables from fabricated ones. Every one of these is the same
   missing primitive: **independently-verifiable provenance for agent actions.**

There is no incumbent standard. This is a greenfield trust layer at the exact moment
the volume of agent work is going vertical.

## The wedge

We do not start by selling a platform. We start by giving away a primitive that is
*obviously* useful and *impossible to fake*:

```
  capture            →   tamper-evident ledger     →   Gold Seal           →   re-verify
  (zero-config hook)     (hash-chain + causal DAG)     (signed certificate)     (anybody, offline)
```

- **Capture is free and zero-config.** A Claude Code hook records every tool call
  into a per-session ledger. No SDK, no rewrite. (Shipped: Phase 1.)
- **The ledger is tamper-evident by construction.** Hash-chained, HLC-ordered,
  Ed25519-signable. Edit one byte and verification localizes the break to the exact
  step. (Shipped: `korg-ledger@v1`, three conformant implementations.)
- **The Gold Seal makes it portable and human.** One JSON object: the events, an
  issuer signature, and a human-legible summary that is **re-derived from the events**
  — so the "files touched / tools used / steps" a person reads literally cannot lie.
  (Shipped: `goldseal@v1`, this work.)
- **Re-verification needs nothing of ours.** A 200-line dependency-light Rust binary,
  a stdlib Python module, or a single browser tab (Web Crypto) all check a seal
  byte-identically. **Zero trust in the tool that produced it.** That is the whole
  product.

The thing a user shares — a green seal on a PR, a verifiable session attached to a
deliverable — is itself the distribution. Every shared seal is an ad for the format,
and every recipient who verifies it has installed nothing.

## Why it's a moat: the standard play

The defensibility is not the verifier — it's **adoption of the format**. The
reference points are SSL/TLS, SPDX (software bills of materials), and SLSA (supply-chain
provenance): an open spec plus an independent verifier becomes the thing everyone
checks against, and the network effects accrue to whoever defines and stewards it.

- **Open spec, multiple independent implementations.** Trust comes from *not* having
  to trust us. Three conformant codepaths (Rust/Python/JS) verifying one frozen oracle
  is the credibility, not a closed SaaS.
- **The value moves up the stack.** Once seals are everywhere, the monetizable layers
  are the ones a free verifier doesn't give you: hosted transparency logs and
  timestamping, issuer-identity/registry (key ↔ org binding), org-wide policy and
  audit dashboards, marketplace escrow on verified work, compliance exports.
- **Switching cost is the installed base of seals.** A competitor can copy the verifier
  in a weekend; they cannot copy the millions of already-issued, already-trusted seals
  pointing at our spec.

## Where it lands (concrete)

- **Verifiable PRs / deliverables** — attach a seal; a reviewer (or a buyer on a
  freelance-agent marketplace) confirms exactly what the agent did before trusting it.
- **Enterprise agent audit trails** — every agent action provably recorded; "show me
  what the agent touched in prod last Tuesday" becomes a verifiable query, not a log
  you have to believe.
- **Compliance & assurance** — a portable, tamper-evident artifact for AI-governance
  regimes that is checkable by an auditor who trusts none of the parties.
- **Agent marketplaces** — distinguish genuine, reproducible agent work from
  fabricated output; price and escrow against verified provenance.

## What we do NOT claim (yet)

Honesty is the brand — an earlier internal audit flagged and removed overclaimed
features, and that discipline is the reason anyone should believe the rest of this.

- A Gold Seal proves a session is a **faithful record**, not that the work is **good**.
- It proves **authorship by a key**, not **identity** — key↔real-world-org binding is
  pinned by the relying party today; a hosted registry is future work, not a claim.
- **Time** is proven by an explicit, opt-in network step, not offline. `korg-seal
  resolve` fetches a git-tip anchor's public commit and confirms it introduced the
  tip — a "the chain existed no later than `<commit date>`" bound (demoed live against
  the public repo). Not claimed: a fully-offline timestamp or a trusted-clock
  guarantee stronger than "this was in public git by then."
- The hosted layers above (transparency log, registry, dashboards, marketplace) are
  the *business*, and are **not built yet**. What is built and verifiable today is the
  primitive: capture → ledger → Gold Seal → independent re-verification.

The bet: own the open primitive for verifiable agent work, and the trust network is
the company.
