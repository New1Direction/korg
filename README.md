# korg — Autonomous Software Engineering Runtime

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/MIT)
[![Rust 2021](https://img.shields.io/badge/rust-2021-93450a.svg?style=flat-square)](https://www.rust-lang.org)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg?style=flat-square)](https://github.com/example/grok-acp-harness)
[![Security Audit](https://img.shields.io/badge/audit-passing-brightgreen.svg?style=flat-square)](https://github.com/example/grok-acp-harness)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg?style=flat-square)](https://github.com/example/grok-acp-harness)
[![Discord](https://img.shields.io/discord/872512263093514240?color=%237289DA&label=community&logo=discord&logoColor=white&style=flat-square)](https://discord.gg/xai)

**Korg** is a zero-trust, cryptographically-verifiable runtime for autonomous multi-agent software engineering swarms. It operates on a **Blackboard Architecture** modeled as a **Gravitational Well**, where specialized agents (Architect, Coder, Tester) concurrently propose, validate, and debate code modifications against a shared, observable state.

Every transaction is physically recorded onto a local, content-addressed **Merkle Directed Acyclic Graph (DAG)**, creating an immutable ledger of AI reasoning. When consensus fails or security policies are breached, Korg intercepts the swarm, opening a **Zero-Overlap Inline Security Gateway** that allows human operators to inject criteria or execute a **Playhead Steering Fork**, physically resetting the codebase to any prior state signature.

---

## ⚡ Live Operator Dashboard

The Korg runtime provides a high-tempo, real-time TUI dashboard for complete operational observability. It is designed with a **zero-overlap** inline layout, ensuring critical telemetry and log streams are never obscured by modal dialogs.

```ascii
  ┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
  │ 𝗸𝗼𝗿𝗴 𝘃𝟬.𝟭.𝟬 │ session: 018f7b11-a8b1-7002-88b1-04b128a7b128 │ [●] TELEMETRY ACTIVE             │
  ├──────────────────────────────────────────┬───────────────────────────────────────────────────────┤
  │ 🗂️  SWARM PLAN & STATUS                  │ ⛓️  LIVE MERKLE DAG & PROVENANCE                        │
  │    ├─ [●] Captain   [PLANNING]          │    (tx_00) → (tx_01) → [tx_02] → (tx_03)                │
  │    ├─ [●] Harper    [RESEARCH]          │                                                        │
  │    ├─ [●] Benjamin  [SYNTHESIS]         │ 📊 REAL-TIME TELEMETRY                                 │
  │    └─ [○] Lucas     [IDLE]              │    ├─ Velocity  [ 85.2 t/s]  ▇▆▄▂█▃▅▆                  │
  │                                          │    ├─ Risk      [  0.18   ]  ▂▃▄▃▂▂▂                   │
  │ 💎 ZERO-OVERLAP WORKSPACE                │    ├─ Progress  [ 68.7 % ]  ▂▃▄▅▆▇▇█                  │
  │    ├─ 🟡 Amber Security Gate [IDLE]     │    └─ Entropy   [  0.451  ]  ▄▃▂▃▄▅▄▃                  │
  │    ├─ 🟢 Emerald Consensus   [ACTIVE]   │                                                        │
  │    │  └ Approve Swarm Contract? [Y/n]   │ 📜 STDOUT CONSOLE STREAM                               │
  │    └─ 🔵 Cyan Steering Fork  [IDLE]     │    [Leader] Spawning 4 concurrent persona workers...   │
  │                                          │    [Benjamin] Synthesizing patch for src/db.rs...      │
  │ 📝 ACTIVE WORKSPACE: src/leader.rs       │    [Lucas] Running adversarial test suite...           │
  │    22 | let mut arena_outcome = self.    │    [Evaluator] HARSH FAIL — high semantic churn.       │
  │    23 |     run_arena(&results).await;   │    [Leader] REVISION requested by Evaluator critic.    │
  └──────────────────────────────────────────┴───────────────────────────────────────────────────────┘
```

---

## 🏗️ Architecture & Core Theoretical Pillars

Korg is engineered for deterministic, high-assurance software synthesis. Its architecture is founded on three core mathematical and cryptographic principles.

| Pillar | Description |
| :--- | :--- |
| **Semantic Contract Negotiation** | Swarm actions are governed by contracts negotiated between agents. Acceptance is determined not by keywords, but by the cosine similarity of BERT embeddings between the proposed action and the established goal, ensuring semantic alignment. |
| **Canonical Merkle-DAG Ledger** | Every state change is serialized into a content-addressed transaction block using **RFC 8785 (JCS)** canonicalization. These blocks are chained via parent hashes into a Merkle-DAG, creating a tamper-proof, auditable history of the swarm's entire thought process. |
| **Fail-Secure Visual Firewall** | Korg's agents operate with full GUI context, continuously taking screenshots. A fail-secure OCR firewall scans these images for sensitive patterns (API keys, PII). If a leak is detected, the image is redacted *before* being committed to the log stream, preventing accidental data exposure. |

### Mathematical Foundations (LaTeX)

1.  **BERT Cosine Contract Negotiation:** An action contract `C` is accepted against a goal `G` only if the cosine similarity of their BERT embeddings `E(·)` exceeds a negotiated threshold `τ`.

    $$
    \text{Accept}(C, G) := \frac{E(C) \cdot E(G)}{\|E(C)\| \|E(G)\|} \ge \tau_{\text{accept}}
    $$

2.  **RFC 8785 Canonical Merkle-DAG Chain Serialization:** The hash of a transaction `tx_n` is the SHA-256 digest of its canonicalized payload `JCS(tx_n^{\text{payload}})` XORed with the aggregate hash of its `k` parents, ensuring content-addressability and structural integrity.

    $$
    H(tx_n) = \text{SHA-256} \left( \text{JCS}(tx_n^{\text{payload}}) \oplus \bigoplus_{i=1}^{k} H(tx_{p_i}) \right)
    $$

3.  **Fail-Secure Visual OCR Firewall:** The redaction function `R` is applied to every captured image `I`. It is redacted if any regular expression `p` from the policy set `P` matches the `OCR(I)` text content.

    $$
    R(I, P) = \begin{cases} \text{REDACT}(I) & \text{if } \exists p \in P \text{ s.t. } p \text{ matches } \text{OCR}(I) \\ I & \text{otherwise} \end{cases}
    $$

---

## 🚀 Quick Start

### 1. Clone & Build

Clone the repository and build the `korg` binary. This will compile all Rust components, including the Axum web server, Ratatui TUI, and the core orchestration logic.

```bash
git clone https://github.com/example/grok-acp-harness.git
cd grok-acp-harness
cargo build --release
```

### 2. Launch a Campaign (Web Cockpit)

Initiate an autonomous software engineering campaign and launch the real-time web cockpit to monitor swarm activity, telemetry, and security gates.

```bash
cargo run --release -- campaign --web --prompt "Refactor the authentication layer to use JWTs"
```

Once active, navigate your browser to **`http://localhost:8080`**.

### 3. Launch a Campaign (Terminal UI)

For a pure terminal-based experience, launch the campaign with the Ratatui-based TUI dashboard.

```bash
cargo run --release -- campaign --tui --prompt "Optimize the database connection pool"
```

### 4. Verify Provenance

After a campaign, you can cryptographically verify the integrity of the entire execution trace using the generated attestation certificate.

```bash
cargo run --release -- verify-provenance /tmp/korg/campaigns/<session-id>/provenance-attestation.json
```

---

## ⚖️ Comparative Analysis: Korg vs. Traditional AI Tools

Korg introduces a paradigm shift from simple code completion to a fully autonomous, secure, and observable software engineering environment.

| Capability | korg Swarm Runtime | Traditional AI IDEs (e.g., Cursor) | Standard CLI Bots |
| :--- | :---: | :---: | :---: |
| **Autonomous Execution**<br/><sub>(Writes, builds, tests, and heals code)</sub> | <g-emoji class="g-emoji" alias="heavy_check_mark" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png">✔️</g-emoji> | <g-emoji class="g-emoji" alias="heavy_minus_sign" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2796.png">➖</g-emoji> | <g-emoji class="g-emoji" alias="heavy_minus_sign" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2796.png">➖</g-emoji> |
| **Built-in Adversarial Testing**<br/><sub>(Tests changes in isolated sandboxes)</sub> | <g-emoji class="g-emoji" alias="heavy_check_mark" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png">✔️</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> |
| **Zero-Trust Security Guardrails**<br/><sub>(Visual OCR firewall for secrets)</sub> | <g-emoji class="g-emoji" alias="heavy_check_mark" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png">✔️</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> |
| **Cryptographically Verifiable Ledger**<br/><sub>(Tamper-proof Merkle-DAG history)</sub> | <g-emoji class="g-emoji" alias="heavy_check_mark" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png">✔️</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> |
| **Playhead Steering Forks**<br/><sub>(Rollback and redirect swarm execution)</sub> | <g-emoji class="g-emoji" alias="heavy_check_mark" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png">✔️</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> | <g-emoji class="g-emoji" alias="x" fallback-src="https://github.githubassets.com/images/icons/emoji/unicode/274c.png">❌</g-emoji> |

---

## ⚙️ Core Technologies

Korg is built on a foundation of high-performance, memory-safe, and concurrent systems technologies.

- **[Rust](https://www.rust-lang.org/)**: The core language, providing memory safety and zero-cost abstractions for high-performance systems programming.
- **[Tokio](https://tokio.rs/)**: An asynchronous runtime for writing reliable, non-blocking network and system applications.
- **[Axum](https://github.com/tokio-rs/axum)**: A hyper-performant, ergonomic web framework for building the real-time operator cockpit and SSE telemetry streams.
- **[Ratatui](https://ratatui.rs/)**: A library for building rich, interactive terminal user interfaces, powering the TUI dashboard.
- **[Candle](https://github.com/huggingface/candle)**: A minimalist ML framework from Hugging Face, used for the optional local BERT embedding backend that powers semantic contract negotiation.
- **[ed25519-dalek](https://crates.io/crates/ed25519-dalek)**: A pure-Rust implementation of the Ed25519 digital signature algorithm, used for signing and verifying all cryptographic attestations.