# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

To report a security issue, email the maintainers directly via the contact
listed on the GitHub profile, or open a
[GitHub Security Advisory](https://github.com/New1Direction/korg/security/advisories/new).

Include:
- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested mitigations

You will receive a response within **72 hours**. We will work with you to
understand the issue and coordinate a fix before public disclosure.

## Scope

Security issues in scope:
- Cryptographic provenance attestation (`src/provenance.rs`)
- Ledger integrity and append-only guarantees (`src/registry/log.rs`)
- HLC ordering invariants under adversarial clock conditions
- API key / credential handling in `korg.toml` parsing

Out of scope:
- Issues in LLM providers (OpenAI, Anthropic, Google) themselves
- Theoretical attacks requiring physical access to the host
- Denial-of-service via extremely large prompts

## Disclosure Policy

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure).
We aim to release a patch within **14 days** of a confirmed vulnerability report.
