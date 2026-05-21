# Korg Release Notes

## 🏷️ v1.0.0 — "Swarm Brains Wired" (2026-05-21)

We are thrilled to announce the official **v1.0.0 release of Korg / Yvaeh Mode**! 

This release marks the successful completion of **Phase 1 of our product roadmap**, delivering a production-grade, highly resilient Autonomous Software Engineering Environment (ASEE) runtime. The swarm is alive, collaborative, adversarial, and observable.

---

## 🚀 Key Features in this Release

### 1. Model-Agnostic LLM Core (`src/llm.rs`)
*   **Pure HTTP zero-SDK core**: Built-in HTTP client bypassing heavy external SDK wrappers.
*   **Broad Provider Support**: Out-of-the-box integration with **OpenAI**, **Anthropic Claude** (Message payloads with server-sent event stream parsers), **xAI Grok**, and **Local Ollama**.
*   **Production Resiliency**: Standardized `ResilientLlmProvider` wrapping client calls in an stateful exponential backoff retry loop backed by a custom thread-safe `CircuitBreaker`.
*   **Deterministic Simulation**: Stateful `MockProvider` delivering pre-queued responses for offline testing.

### 2. Multi-Persona Swarm Topology (`src/personas.rs`)
*   **5-Persona Strategies**: Core swarm consisting of:
    *   `Captain` (Planner and Orchestrator)
    *   `Harper` (Codebase Auditor)
    *   `Benjamin` (Builder and Implementer)
    *   `Lucas` (Swarm Critic and Synthesizer)
    *   `Evaluator` (Heavy Adversarial Guardrail)
*   **Prompt-Driven Personas**: Highly-customized, research-grade system prompts read dynamically from `/Prompts/` on the filesystem.

### 3. Closed-Loop Adversarial Contract Negotiation
*   **Cosine Similarity Scoring**: Computes mathematical semantic similarity between proposed criteria and user prompts in real-time.
*   **Local BERT Embeddings**: Integrates Hugging Face's `all-MiniLM-L6-v2` BERT model via **Rust Candle** to run high-fidelity offline vector math.
*   **3-Round Negotiation Lifecycle**: Multi-round negotiation preventing campaign execution until goals are mathematically verified ($\text{similarity} \ge 0.42$ and $\ge 3$ criteria).

### 4. High-Contrast Electric Cockpit UI (`src/tui.rs`)
*   **Vibrant 6-Pane Ratatui Layout**: 24-bit TrueColor neon dashboard displaying concurrent write-locks, active terminals, semantic entropy gauges, and a transaction timeline.
*   **Time-Travel Scrubber**: Arrow-key driven playhead scrubbing enabling operators to navigate backward and forward along the transaction logs.
*   **F-Key Swarm Branching**: Interactive steering terminals for committing text buffers and copy-branching sandboxed workspaces at target playhead checkpoints.
*   **Zero-Trust Overrides**: Magenta flashing security intercept cards gating contested actions against declarative policies.

### 5. Resilient Transactional Memory
*   **`.ktrans` Ledger**: All Blackboard state modifications are serialized into signed transaction blocks.
*   **Crash Self-Healing**: Automated process crash detection that stalls loops, sweeps `.ktrans` records, rehydrates Blackboard memory, and re-spawns workers to continue execution without state loss.

---

## 🧪 Verification & Stability Metrics

*   **100% Offline Crate Stability**: The entire runtime compiles and runs completely offline with zero dependency warnings.
*   **Passing Test Suite**: **21/21 unit tests pass successfully** inside `1.99` seconds.
*   **Validated Platforms**: Successfully built and tested on macOS (Arm64), Linux (x86_64), and Docker scratch containers.

---

## 🔮 What's Next in Phase 2

As we transition into the next phase of the Korg roadmap, our priorities focus on expanding accessibility and increasing cognitive bandwidth:

1.  **Public Web Cockpit (`https://yvaehkorg.lol`)**:
    *   Transitioning from TUI to a gorgeous glass-morphism web client.
    *   Streaming Blackboard CRDT transaction events via real-time WebSockets/SSE.
2.  **Multi-Modal Vision Integration**:
    *   Extending the `LlmProvider` traits to support image payloads (e.g. wireframes, UI mockups).
    *   Enabling Harper and Benjamin to inspect user interfaces and suggest design improvements visually.
3.  **Distributed Swarm Clusters**:
    *   Scaling execution from local multi-processing to multi-node clusters.
    *   Implementing formal verification protocols to check code correcteness across large agent fleets.

---

*Thank you to all our core maintainers and operators who helped wire the Korg swarm brains. Let's build the future of autonomous engineering!*
