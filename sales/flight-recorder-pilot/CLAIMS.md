# Claims and evidence

Every technical statement in this kit must appear in this file with the public
source that backs it. If a claim is not listed here, do not make it.

**Baseline:** `main` at
[`781bc41`](https://github.com/New1Direction/korg/commit/781bc414372d8f15a612160cc4844e08767bfbe5)
(korg-ledger@v1 revision 1.1). Redaction (`korg-redact@v1`), the lint commands,
`rechain-redacted`, `--require-redacted`, and causal-link checking of nested
journals landed in that commit after release
[v0.2.0](https://github.com/New1Direction/korg/releases/tag/v0.2.0), and are
listed as *Unreleased* in the [changelog](../../CHANGELOG.md). **Every pilot pins
a commit at or after `781bc41`, never the v0.2.0 tag.** Tell the customer that
this code is recent.

## Claims you may make

| # | Claim | Evidence |
|---|---|---|
| C1 | Claude Code sessions are captured through `PostToolUse`, `Stop`, and `SubagentStop` hooks into one ledger per session at `~/.korg/sessions/<session_id>.jsonl`. The agent does not need to be instructed to log anything. | [`adapters/claude-code/README.md`](../../adapters/claude-code/README.md#what-it-produces), [`hook.py`](../../adapters/claude-code/src/claude_code_adapter/hook.py), [`claude_settings.py`](../../adapters/korg-setup/src/korg_setup/claude_settings.py) |
| C2 | The hook catches its own errors and always exits 0, so a capture failure does not fail the Claude Code session. It runs synchronously, so it does add time (see L12). | [`hook.py`](../../adapters/claude-code/src/claude_code_adapter/hook.py) module docstring |
| C3 | Historical Claude Code sessions already on disk can be backfilled into the same verifiable per-session format with `korg-backfill`, which can be limited to one project directory with `--projects-dir`. | [`adapters/claude-code/README.md`](../../adapters/claude-code/README.md#one-shot-backfill-of-history), [`backfill_cli.py`](../../adapters/claude-code/src/claude_code_adapter/backfill_cli.py) |
| C4 | Each ledger is a SHA-256 hash chain (`korg-ledger@v1`). Given a trusted tip, any edit, deletion, insertion, or reorder is detected and localized to a `seq_id`. The format also supports HMAC-keyed chains, which cannot be forged without the key; the Claude Code hook does not use them today. | [`spec/korg-ledger-v1/README.md`](../../spec/korg-ledger-v1/README.md), [`SPEC.md`](../../spec/korg-ledger-v1/SPEC.md) §3–5 |
| C5 | Causal links (`triggered_by`) form a checked DAG: prompt → model turn → tool call. Forward, dangling, and malformed links fail verification. | [`SPEC.md`](../../spec/korg-ledger-v1/SPEC.md) §9 |
| C6 | There are three independent verifier implementations (Rust, Python, JavaScript) that pass one shared conformance vector set and are differentially fuzzed against each other, with 0 divergences reported. | [`spec/korg-ledger-v1/README.md`](../../spec/korg-ledger-v1/README.md#conformant-implementations), [`CHANGELOG.md`](../../CHANGELOG.md) |
| C7 | Zero-install browser verifiers check a file locally in the page and do not upload it. The pages load web fonts from Google Fonts. | [Ledger verifier](https://new1direction.github.io/korg/web/index.html), [Certificate verifier](https://new1direction.github.io/korg/web/seal.html), [`seal.html`](../../spec/korg-ledger-v1/web/seal.html) |
| C8 | Secrets matching `korg-redact@v1` rules (sensitive key names, bearer tokens, provider/GitHub/Slack/AWS/Google keys, JWTs, PEM private keys, URL credentials, `NAME_TOKEN=…` assignments) are replaced with `[REDACTED:<rule>]` *before* hashing. This is on by default in `LedgerWriter`, which the Claude Code hook uses. | [`REDACTION.md`](../../spec/korg-ledger-v1/REDACTION.md), [`writer.py`](../../adapters/korg-ledger-py/src/korg_ledger/writer.py), [`canonical_emit.py`](../../adapters/claude-code/src/claude_code_adapter/canonical_emit.py) |
| C9 | `python -m korg_ledger lint` (ledgers) and `korg-verify --require-redacted` (ledgers and Certificates) flag any persisted secret the rules recognize. `python -m korg_ledger rechain-redacted` migrates a leaked journal to a new, clean chain. | [`REDACTION.md`](../../spec/korg-ledger-v1/REDACTION.md), [`adapters/korg-ledger-py/README.md`](../../adapters/korg-ledger-py/README.md), [`crates/korg-verify/README.md`](../../crates/korg-verify/README.md) |
| C10 | A `korgcert@v1` Certificate embeds a session's full event chain, a summary re-derived from those events (agents, tool counts, file paths from file-tool arguments, seq range), and an Ed25519 issuer seal. Verifiers reject a summary that disagrees with the events, a missing seal, and (when pinned) the wrong issuer key. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §1–5, [`adapters/korg-seal/README.md`](../../adapters/korg-seal/README.md) |
| C11 | `korg-seal mint` refuses to seal a chain that does not verify. | [`adapters/korg-seal/README.md`](../../adapters/korg-seal/README.md#use) |
| C12 | A Certificate can be anchored to a public GitHub commit that adds a line containing the anchored hash. `korg-seal resolve` fetches the commit and confirms it introduced the hash. That shows the tip was published in that commit. The commit date is self-asserted by git, so the dependable bound is "before any third party fetched or mirrored that commit." | [`adapters/korg-seal/README.md`](../../adapters/korg-seal/README.md#trusted-time--anchor-to-a-public-commit-then-resolve), [`resolve.py`](../../adapters/korg-seal/src/korg_seal/resolve.py), [`SPEC.md`](../../spec/korg-ledger-v1/SPEC.md) §8.2 |
| C13 | A composite GitHub Action verifies Certificates and ledgers in CI (the verification itself makes no network calls), optionally pins the issuer key, writes a job summary, optionally posts a PR comment, and fails the job on any tampered or unverifiable file. | [`.github/actions/verify-korgcert/action.yml`](../../.github/actions/verify-korgcert/action.yml), [`korgcert-demo.yml`](../../.github/workflows/korgcert-demo.yml) |
| C14 | The same ledger format ingests traces from OpenAI Codex CLI (WebSocket frames) and Grok Heavy (NDJSON) through library adapters with tests. | [`README.md`](../../README.md#validated-across-three-vendors-n3), [`adapters/codex-ws/README.md`](../../adapters/codex-ws/README.md), [`adapters/grok-heavy/README.md`](../../adapters/grok-heavy/README.md) |
| C15 | Korg is open source under MIT or Apache-2.0. There is no license fee. | [`LICENSE`](../../LICENSE), [`LICENSE-APACHE`](../../LICENSE-APACHE) |

## Which verifier checks what

"Independent" means a different implementation. The Node.js CLI and the browser
pages run the same `verify.mjs`, so together they count as **one** (JavaScript).

| Check | Rust | Python | JavaScript (Node CLI or browser) |
|---|---|---|---|
| Ledger chain + causal DAG | `korg-verify <ledger>` | `python -m korg_ledger lint <ledger>` | `node spec/korg-ledger-v1/js/bin.mjs <ledger>` or the [ledger page](https://new1direction.github.io/korg/web/index.html) |
| Certificate (chain, summary, seal, pinned key) | `korg-verify <cert> --pubkey <hex>` | `korg-seal verify <cert> --pin <hex>` | `node spec/korg-ledger-v1/js/bin.mjs <cert> --pubkey <hex>` or the [Certificate page](https://new1direction.github.io/korg/web/seal.html) |
| Redaction lint | `korg-verify --require-redacted` (ledgers and Certificates) | `python -m korg_ledger lint` on a ledger. It misreads a Certificate file as a single event, so for a Certificate run it on the extracted events: `jq -c '.events[]' cert.json > events.jsonl` | none |

`python -m korg_ledger lint` reports chain, DAG, and redaction problems together.
When comparing verdicts across verifiers, compare chain and DAG results only; a
redaction finding is a separate check.

## Limits to disclose

Say these before the customer finds them.

| # | Limit | Evidence |
|---|---|---|
| L1 | **Live capture covers Claude Code only.** The Codex CLI and Grok Heavy adapters are libraries that translate recorded traces. They have no CLI, and their documented output path posts to `korg-server`, which the pilot does not deploy. Recording Codex means intercepting its WebSocket traffic (for example with mitmproxy), which also captures credentials. The pilot therefore treats Codex as a feasibility replay that needs a short glue script. No adapter exists for Copilot, Cursor, Windsurf, or Gemini CLI. | [`adapters/codex-ws/README.md`](../../adapters/codex-ws/README.md) |
| L2 | **An unkeyed chain is tamper-evident, not tamper-proof.** Whoever controls the ledger file can rewrite and rehash the whole chain. That includes the agent itself, whose shell tool runs as the same OS user. Detection needs a trusted tip held elsewhere: a sealed Certificate that has left the machine, an external anchor, or an HMAC key the writer does not hold. Anything rewritten *before* the tip is fixed goes undetected, so the time between capture and sealing is a real exposure. | [`SPEC.md`](../../spec/korg-ledger-v1/SPEC.md) §8.2 honest limits |
| L3 | **Signatures prove a key, not a person or an organization.** The relying party pins the issuer public key. There is no hosted key registry and **no revocation**: a leaked key's past seals stay valid for anyone who still pins it. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §1 and "No revocation" |
| L4 | **No trusted timestamp.** `issued_at` and `anchored_at` are local clocks, and a git commit date is self-asserted. Public anchoring needs a public GitHub repository, because `resolve` uses the unauthenticated GitHub API. The top-level README lists a "live network anchoring resolver (trusted-time witness)" as not shipped; `korg-seal resolve` exists but gives only the weaker bound in C12. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §1, [`resolve.py`](../../adapters/korg-seal/src/korg_seal/resolve.py), [`README.md`](../../README.md#status) |
| L5 | **A Certificate proves a faithful record, not good work.** The `claim` field is free prose that the issuer signs but no machine checks. A Certificate is also not bound to a git commit or PR diff. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §1 |
| L6 | **Redaction is rule-based, not DLP.** It catches the listed patterns and sensitive key names. Customer-specific secret formats, personal data, and proprietary code are recorded as-is. | [`REDACTION.md`](../../spec/korg-ledger-v1/REDACTION.md) §3 |
| L7 | **The `stream-json` adapter writes large payloads to `.korg/blobs/` without redaction.** The pilot does not deploy it. | [`adapters/stream-json/README.md`](../../adapters/stream-json/README.md) |
| L8 | **Local, single-user trust boundary.** `korg-server` and `mcp_server.py` must not run on a shared or untrusted network. The pilot does not deploy them. | [`README.md`](../../README.md#what-korg-does) trust-boundary warning |
| L9 | **Capture records what the Claude Code transcript shows.** That means prompts, model turns, tool calls, and results. It does not see actions outside Claude Code, and it does not block or approve actions. | [`hook.py`](../../adapters/claude-code/src/claude_code_adapter/hook.py), [`adapters/claude-code/README.md`](../../adapters/claude-code/README.md#causal-mapping) |
| L10 | **Packaging is source-based, and the published packages are stale.** `korg-verify` 0.1.0 on crates.io and `@korgg/ledger-verify` 0.1.0 on npm (both June 2026) predate Certificates and redaction, so the pilot does not use them. Nothing is on PyPI. `claude-code-adapter` declares a dependency on `korg-ledger-py`, a name that was not registered on PyPI as of 2026-10-13. To avoid dependency confusion, install from a local checkout with `--no-deps`. The release ships `korg` binaries only, so `korg-verify` is built from source. | [`README.md`](../../README.md#status), [`adapters/claude-code/pyproject.toml`](../../adapters/claude-code/pyproject.toml), [v0.2.0 release](https://github.com/New1Direction/korg/releases/tag/v0.2.0) |
| L11 | **Platform.** The hook uses POSIX file locks and CI runs on Linux. macOS and Linux workstations are in scope; Windows is not validated. | [`hook.py`](../../adapters/claude-code/src/claude_code_adapter/hook.py) (`fcntl`), [`ci.yml`](../../.github/workflows/ci.yml) |
| L12 | **Hook overhead is unmeasured.** The hook reparses the whole session transcript on every firing, synchronously, so its cost grows with session length. The pilot measures it rather than promising a number. | [`hook.py`](../../adapters/claude-code/src/claude_code_adapter/hook.py) |
| L13 | **Capture fidelity.** Tool results over 8,000 characters are truncated. Transcript wall-clock timestamps are not carried into the ledger, which keeps causal order but not absolute timing. Subagent (sidechain) events are flattened into the parent session. Events that fail a causality or redaction check are dropped rather than written, which shows up as a coverage difference. | [`adapters/claude-code/README.md`](../../adapters/claude-code/README.md#known-limitations), [`canonical_emit.py`](../../adapters/claude-code/src/claude_code_adapter/canonical_emit.py) |
| L14 | **The summary's `files` list is not "every file changed."** It lists `file_path`/`path` arguments of tool calls, so files changed through shell commands (`sed -i`, `rm`, `git mv`) are missing and files that were only read are included. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §4 |
| L15 | **Certificates contain the whole session.** A Certificate embeds every event, so it is as sensitive as the ledger. Never publish one to anchor it; publish only the hash. | [`KORGCERT.md`](../../spec/korg-ledger-v1/KORGCERT.md) §2 |
| L16 | **Hook scope is set by where it is registered.** A hook in the user-level `~/.claude/settings.json` records every Claude Code session on the machine, in every repository. The pilot registers it per pilot repository instead, in `.claude/settings.local.json`, which Claude Code reads for that project only. A hand-created copy of that file must be added to `.gitignore`. | [`claude_settings.py`](../../adapters/korg-setup/src/korg_setup/claude_settings.py) (user-level default); Claude Code [settings](https://code.claude.com/docs/en/settings) and [hooks](https://code.claude.com/docs/en/hooks) docs (project-local settings) |
| L17 | **`korg-setup` is not used in the pilot.** It requires `korg-recall-mcp` on the PATH (even for `--dry-run`), always registers the recall MCP server, and writes a backup of `~/.claude.json`, which holds Claude Code login state. The pilot registers the hook by hand. | [`setup.py`](../../adapters/korg-setup/src/korg_setup/setup.py), [`adapters/korg-setup/README.md`](../../adapters/korg-setup/README.md#safety) |

## Questions prospects ask about the repository README

- *"Your README says agents must be instructed to log, and that fully passive
  capture is on the roadmap."* The README's wording there is broader than the
  current state. The note sits in the MCP-sink section, and the roadmap item covers
  agents in general. The Claude Code hook, documented in the
  [claude-code adapter README](../../adapters/claude-code/README.md#what-it-produces)
  and listed as shipped in the same Status section, needs no agent cooperation (C1).
  Other agents still do, or have no capture path (L1). Say this plainly instead of
  arguing about wording.
- *"The README says the anchoring resolver isn't shipped."* The trusted-time
  witness isn't shipped. `korg-seal resolve` exists and gives the weaker,
  publication-based bound in C12 (L4).
- *"The browser page offers a 'Gold Seal – verified' badge, and the CI action
  comments 'Certificate verified'."* "Verified" there means the record passed
  cryptographic checks. It is not a compliance mark, and the pilot does not use
  badges in customer-facing material.

## Claims you must not make

- "Korg makes you SOC 2 / ISO 27001 / EU AI Act compliant," "certified," "audit-ready," or "approved by auditors."
- "Captures every coding agent," "fully passive for all agents," or support for any agent outside L1.
- "Tamper-proof" for an unkeyed chain, "trusted timestamp," "proves when," or "proves who wrote the code."
- "Without trusting the agent" for a session that has not been sealed yet (L2).
- "Lists every file the agent changed" (L14), or that a Certificate is tied to a specific commit or PR (L5).
- "Prevents" or "blocks" unsafe agent actions (this pilot records them).
- Hosted dashboards, transparency logs, key registries, revocation, SaaS, or any roadmap item as available.
- Any performance number not measured in the customer's own pilot.

## Validation log

On 2026-10-13, against `781bc41`, in a clean Linux sandbox:

- The JS verifier reported `journal VALID — 6 events` on the
  [sample session](../../spec/korg-ledger-v1/web/samples/session.jsonl). After one
  line was edited it reported `seq 4: entry_hash mismatch` and exited 1. Deleting
  an event and swapping two events were each reported as `prev_hash breaks the
  chain` by both the JS verifier and `korg_ledger lint`.
- The [sample Certificate](../../spec/korg-ledger-v1/web/samples/korgcert.json)
  verified as `korgcert VALID`. With one file removed from its summary, both JS and
  `korg-seal verify` reported `summary does not match the events`.
- `python -m korg_ledger lint` on the sample Certificate file reported
  `missing entry_hash`, confirming that it cannot read a Certificate directly. On
  the Certificate's events extracted with `jq -c '.events[]'` it reported
  `redaction-clean`. After a bearer token was planted in one event, it flagged
  `unredacted bearer_token` and exited 1.
- JS and Python conformance suites: `PASS`.
- 265 tests passed across `claude-code`, `korg-seal`, `korg-setup`, and `korg-ledger-py`.
- End to end: a synthetic Claude Code transcript containing a bearer token was
  fed to `korg-hook`. The resulting 5-event ledger contained `[REDACTED:bearer_token]`
  and no raw token, and `korg_ledger lint` reported it chain-intact and
  redaction-clean. `korg-seal mint` produced a Certificate that the JS verifier
  accepted with the issuer key pinned and rejected with a different pinned key.

Not run in that sandbox: the Rust `korg-verify` binary (no Rust toolchain was
available), a live Claude Code session, and the GitHub Action. Week 1 of each
pilot covers the first two on the customer's machines; the CI tiers cover the
action.
