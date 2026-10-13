# Security and data-handling terms

These terms describe how the pilot handles customer data. They are written to be
attached to the SOW and read by the customer's security reviewer. If they conflict
with a signed NDA, DPA, or MSA, the signed agreement wins.

## 1. What the pilot records

The Claude Code capture hook is registered only in the pilot repositories, so it
records only Claude Code sessions started there
([L16](CLAIMS.md#limits-to-disclose)). It writes one ledger per session, which can
contain:

- the developer's prompts and the model's responses, as they appear in the session
  transcript;
- every tool call and its arguments, such as file paths, shell commands, and edit
  contents;
- tool results, such as file contents read, command output, and test output
  (each truncated at 8,000 characters);
- session identifiers, the agent label, ledger ordering clocks, and hash-chain
  fields.

**A Certificate embeds the same events as the ledger it was minted from**
([L15](CLAIMS.md#limits-to-disclose)). **Treat ledgers and Certificates as
confidential source code.** They hold much of what the developer's Claude Code
transcript already holds on the same machine.

## 2. Where data lives

- **Customer environment only, by default.** Capture, sealing, and verification run
  on customer workstations and customer CI. Ledgers are written under `~/.korg` on
  the developer's machine.
- **No hosted service.** The pilot deploys no Korg server, collector, or
  dashboard, and it sends no telemetry to us or to anyone else. `korg-server` and
  `mcp_server.py` are not deployed, because Korg v1 is designed for local,
  single-user workspaces ([L8](CLAIMS.md#limits-to-disclose)).
- **Browser verifiers run locally.** The hosted verifier pages check the file in
  the reviewer's browser and do not upload it. The pages do load web fonts from
  Google Fonts. Customers who prefer can serve the same pages from their own host.
- **Network access by Korg tooling during the pilot** is limited to: cloning the
  public Korg repository; installing Python build and runtime dependencies from
  the customer's package index; fetching Rust crates from crates.io (or a mirror)
  to build `korg-verify`; GitHub Actions fetching the pinned action; loading the
  hosted verifier pages, if used; the CI action's PR comment through the GitHub
  API, if enabled; and, only if the customer opts in, `korg-seal resolve` calling
  the public GitHub API for an anchored commit. Ledger content is never sent in
  any of these.

## 3. Vendor access

- **No standing access.** We do not ask for accounts on customer repositories,
  workstations, or CI. Work happens over customer-hosted screen-share, or in a
  customer-provisioned sandbox if the customer chooses one.
- **Customer-approved sharing only.** We receive only artifacts the customer has
  reviewed and approved, at the level chosen in
  [questionnaire item 22](05-discovery-questionnaire.md#e-data-sensitivity):
  nothing, verifier output and summaries, selected Certificates (which contain full
  session content), or sandbox access.
- **Approved artifacts** travel over a customer-chosen channel (for example the
  customer's file share). We do not upload them to public repositories, issue
  trackers, or other third-party services.
- **No AI processing of customer data** without written approval. We will not paste
  customer ledgers, transcripts, or code into third-party AI services.
- **Bug reports upstream** never include customer data. Reproductions use synthetic
  data. Security issues go through the private GitHub Security Advisory channel
  in Korg's [SECURITY.md](../../SECURITY.md). That file's supported-versions table
  predates v0.2.0, so we report through that channel whatever the version.

## 4. Secrets and redaction

- **Redaction before hashing.** The hook writes through `LedgerWriter`, which
  applies the `korg-redact@v1` policy before an event is hashed or written. It
  replaces recognized secrets with `[REDACTED:<rule>]` markers that carry no
  hash, prefix, or length of the secret ([C8](CLAIMS.md#claims-you-may-make)).
- **Redaction is rule-based, not DLP.** It covers sensitive key names and the
  listed credential patterns. Internal token formats, personal data, customer
  data, and proprietary code are recorded as-is ([L6](CLAIMS.md#limits-to-disclose)).
- **Canary drill.** In the first week we plant synthetic secrets (never real ones)
  in each format from [questionnaire item 21](05-discovery-questionnaire.md#e-data-sensitivity),
  run them through a real session, and record which formats are redacted. Formats
  that are not redacted are reported as findings, with mitigations.
- **Lint.** Every pilot ledger is checked with `python -m korg_ledger lint` or
  `korg-verify --require-redacted`, and Certificates are minted only from ledgers
  that pass. Every Certificate is checked too: with `korg-verify --require-redacted`
  where a Rust toolchain is allowed, otherwise by running the Python lint over the
  Certificate's extracted events. The CI gate runs the same redaction-lint step,
  because the `verify-korgcert` action does not check redaction.
- **Remediation without tampering.** A ledger that is found to contain a secret is
  not edited in place. It is re-chained into a new, clean journal with
  `python -m korg_ledger rechain-redacted`, and the original is deleted under the
  customer's procedure. **The exposed secret must be rotated.** If the secret
  also reached a Certificate that was committed to git or attached to a PR,
  removing it means rewriting that history, which is one reason the CI gate can
  read Certificates from a private store instead.
- **Excluded component.** The `stream-json` adapter stores large payloads in
  `.korg/blobs/` without redaction, so the pilot does not use it
  ([L7](CLAIMS.md#limits-to-disclose)).

## 5. Keys

- **The customer generates and holds the issuer key.** `korg-seal` creates a 32-byte
  Ed25519 seed at `~/.korg/issuer.ed25519` with mode `0600`, or at a path the
  customer chooses. We never receive, copy, or escrow a customer private key.
- **Pinning.** Verification in CI and in the runbook pins the issuer public key. A
  Certificate signed by any other key fails.
- **Who seals, and when.** A session ledger lives on the developer's machine. Until
  it is sealed, it can be rewritten there by anyone with that user's access,
  including the agent itself, since its shell tool runs as the same user
  ([L2](CLAIMS.md#limits-to-disclose)). Once it is sealed into a Certificate and
  that Certificate leaves the machine (committed for CI, stored centrally, or
  anchored), later edits are detected. In the pilot, the customer chooses one of
  two models, and the report records the trade-off and the measured time between
  capture and sealing:
  - **(a) Designated key holder (recommended).** A named person, such as a release
    engineer, seals ledgers that developers submit. Developers never hold the key.
  - **(b) Shared team key.** Each pilot developer seals with a copy of the team key
    at PR time. This is simpler, but it puts one private key on every pilot laptop,
    and any of those developers could rewrite and re-seal a session before
    submitting it.
- **No revocation.** `korgcert@v1` has no revocation mechanism. If a key leaks, its
  past seals stay valid for anyone who still pins it. Recovery means generating a
  new key, pinning it everywhere (CI, runbook, reviewers), and treating seals from
  the old key as untrusted from the leak date onward
  ([L3](CLAIMS.md#limits-to-disclose)).
- **What a signature means.** A valid seal proves that the holder of that key
  signed the record. It does not identify a person or organization; that binding
  is the customer's key custody ([L3](CLAIMS.md#limits-to-disclose)).
- **HMAC keying is not used in the pilot.** The ledger library supports
  HMAC-keyed chains, which cannot be forged without the key, but the Claude Code
  hook does not expose a key setting today. Pinned Certificates, which are held
  outside the developer's machine, provide the trusted tip instead
  ([L2](CLAIMS.md#limits-to-disclose)).

## 6. Publishing and anchoring (opt-in)

Public anchoring commits a small file that contains **only** a Certificate's tip
hash to a **public** GitHub repository. `korg-seal anchor` then binds that commit
into the Certificate, and `korg-seal resolve` lets anyone confirm the commit added
the hash.

- **Never publish the Certificate itself** to anchor it. The `korg-seal` README
  example commits the whole seal; in the pilot we commit only the hash, because a
  Certificate contains full session content ([L15](CLAIMS.md#limits-to-disclose)).
- **What becomes public:** a SHA-256 hash, the commit, and the repository name. A
  hash reveals nothing practical about the session, but the customer decides
  whether to publish at all.
- **What it proves:** the hash was in that public commit. The commit date is
  self-asserted by git, so the dependable bound is "before any third party fetched
  or mirrored the commit." It is not a trusted timestamp
  ([L4](CLAIMS.md#limits-to-disclose)).
- **Without a public repository,** anchoring is structural only and gives no time
  bound.

## 7. Retention and deletion

- **The customer owns all ledgers, Certificates, and reports** and sets their
  retention under its own policy.
- **Vendor copies.** We delete any customer artifacts we hold within 30 days of
  acceptance, or sooner on request, and confirm the deletion in writing. Exception:
  the final report and SOW, which we keep under the NDA as business records unless
  the customer asks us to delete them too.
- **Certificates in git.** If the customer has CI read Certificates committed to a
  repository, their content lives in git history and is subject to that
  repository's retention. Deleting it later means rewriting history.
- **Uninstall.** The runbook includes removing the hook entries. Uninstalling does
  not delete ledgers; the customer deletes or archives them under its retention
  policy.

## 8. Software supply chain

- All installed components are public, MIT / Apache-2.0 Korg code, installed from
  a local checkout at a commit SHA (at or after `781bc41`) recorded in the runbook
  and the report.
- Korg's Python packages are not on PyPI, and `claude-code-adapter` declares a
  dependency on `korg-ledger-py`, a name that was not registered there as of
  2026-10-13. To rule out dependency confusion, the Korg packages are installed
  from the checkout with `--no-deps`. Third-party Python packages (`cryptography`
  and its dependencies, plus the `setuptools` and `wheel` build tools) come from the
  customer's usual index ([L10](CLAIMS.md#limits-to-disclose)).
- Building `korg-verify` pulls its locked Rust dependencies (for example
  `sha2`, `hmac`, `ed25519-dalek`, `serde_json`, `regex`) from crates.io or the
  customer's mirror, pinned by the repository's `Cargo.lock`.
- The stale `@korgg/ledger-verify` npm package and `korg-verify` crates.io release
  (both 0.1.0) are not used. The JavaScript verifier runs from the checkout.
- The runbook lists exact versions of everything installed.
- The redaction, lint, and nested-journal checks the pilot relies on landed on
  `main` after v0.2.0 and are recent. The pilot exercises them on the customer's
  own sessions rather than assuming them.
- The CI action is referenced by commit SHA, not by branch.

## 9. Incident handling during the pilot

If either party finds a secret exposure, unexpected data flow, or a vulnerability
in Korg during the pilot:

1. The finder tells the other party's named contact within one business day.
2. Capture is paused on affected machines if the customer asks.
3. Secret exposures follow section 4 (re-chain, delete, rotate).
4. Korg vulnerabilities go through the private advisory channel in
   [SECURITY.md](../../SECURITY.md), never a public issue.

## 10. No certification

The pilot and its outputs are not a certification, attestation, audit opinion, or
legal advice. We make no representation that using Korg satisfies any law,
regulation, standard, or contract. Draft control-evidence mappings (Evidence Pack)
are working material for the customer and its auditors to review.
