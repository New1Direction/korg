# korg — the autonomous software engineering runtime.

Korg is officially live. It is not another agent framework. Korg is a **production-grade, cryptographically verifiable autonomous software engineering runtime** designed for high-consequence development. 

With all five tracks complete and fully integrated, Korg is now live at **https://yvaehkorg.lol**, delivering zero-trust process isolation, a deterministic Merkle-DAG ledger, and a multi-modal Enterprise Vision Policy Engine.

---

## ⚡ Technical Differentiators & Architecture

### 1. Cryptographically Secure Merkle-DAG Ledger (`src/provenance.rs`)
Unlike traditional LLM harnesses that output mutable history, Korg records every runtime operation, worker thought, tool output, and code delta into a content-addressed, mathematically secure Merkle-DAG.
* **JCS Canonicalization**: Structured JSON payloads are canonicalized under RFC 8785 before hashing to guarantee absolute deterministic consensus.
* **Deterministic Replay**: Every campaign is represented as a chain of cryptographically signed `.ktrans` transaction records, enabling operators to scrub playheads back and forth, auditing the exact cognitive state of the swarm at any tick.
* **ed25519 Authority Signatures**: Every transaction node in the ledger is cryptographically signed by the orchestrating swarm authority key.

### 2. Enterprise Vision Policy Engine & Visual Firewall (`src/vision_policy.rs`)
Multi-modal vision (timeline screenshots) is a powerful capability for testing UI and verifying layout. However, letting LLM workers capture raw prod screens creates massive data leak risks. Korg resolves this with a zero-trust inline visual policy engine:
* **Real-time Interception**: Every image attachment is decoded and filtered through strict regex and text/OCR segment analysis.
* **Automated Redaction Modes**: Infractions trigger immediate `blur`, `placeholder`, or `blackout` transformations before base64 payloads ever reach the SSE stream or public timeline.
* **Operator Interventions**: Violations trigger an interactive **Security Policy Blocked** double-bordered modal (in both TUI and Web cockpits), allowing operators to either Force Override (creating a signed ledger exception) or reject the transaction.

### 3. Git Worktree Isolation & Sandboxing (`src/tools.rs`)
Workers operate inside strict physical sandbox boundaries. Rather than running code modifications in the active directory, Korg isolates work in temporary, ephemeral Git worktrees.
* **Pre-commit Merkle Verification**: Code merges are validated using low-level git indices (`git write-tree`).
* **Zero-Leak Runtimes**: Rollbacks are mathematically absolute; failed execution plans leave the host codebase perfectly pristine.

### 4. Adversarial Swarm Arena & Contract Negotiation (`src/evaluator.rs`)
Code modifications are not committed on a single worker's whim. Changes must pass through a multi-round competitive arena:
* **Five Rubric Evaluations**: Every modification is evaluated against five adversarial rubrics (completeness, correctness, syntax, security, and performance).
* **Semantic Entropy Checking**: Re-evaluates responses to detect cognitive drift and ensure maximum confidence before merging.
* **Contract Attestations**: Multi-persona agents negotiate requirements, and only sign off once all criteria are met.

---

## 🚀 Quick-Start Command Schema

Run a fully observable campaign in your browser with our sleek, pitch-black dashboard:

```bash
# Clone and build
git clone https://github.com/example/korg.git
cd Korg
cargo build --release

# Run demo campaign with real-time web telemetry
./target/release/korg campaign --web
```

Open **`http://localhost:8080`** in your browser to view the **Premium Landing Page**, launch the **Swarm Cockpit**, and audit the **Interactive Provenance Chain Explorer**.

---

## 🔒 Security Compliance

Korg satisfies the **Zero-Trust Visual Compliance Guardrail** by ensuring:
1. No raw sensitive data (e.g. `password`, `api_key`, `secret`, `prod-`) is ever broadcast to public web endpoints.
2. Signed cryptographic audits record all operator-approved visual overrides inside the immutable attestation ledger.
3. Runtimes fail-secure, falling back to complete data blackouts if security evaluations encounter anomalies.

**Korg is stable, secure, and ready for production.**
🔗 Public cockpit live at: **https://yvaehkorg.lol**
