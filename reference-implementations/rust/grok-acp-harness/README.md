# korg

**A clean, production-oriented reference implementation of the Korg Heavy-Tier agent orchestration system.**

`korg` is a complete, working reference implementation of a sophisticated multi-agent orchestration architecture. It includes cryptographically signed messaging (ACP v1.17), rigorous adversarial evaluation, multi-round output aggregation (Arena), tamper-evident transactional memory (`.ktrans`), and a real-time terminal dashboard.

It is designed to be studied, extended, and used as a foundation for serious, long-running autonomous agent systems.

---

## 🌀 The Chaotic Layman Explainer (Explain It Like I'm a Mad Scientist)

If you've ever tried to make multiple AI agents work together, you know it usually turns into a chaotic playground fight where they overwrite each other's code, hallucinate imaginary APIs, and enter infinite loops of agreeing with their own bad ideas until your API wallet is dry.

**Korg is the digital straitjacket, high-frequency flight recorder, and hyper-adversarial boot camp for your AI swarms.**

Instead of letting agents whisper sweet lies to each other, Korg forces them to operate like a high-security cryptographic bank:
* **The Swarm (Lucas, Harper, Benjamin, Sophia)**: A group of specialized AI agents locked in isolated directories. They can't touch each other's files. Benjamin is your builder, Harper is your cynical researcher who believes nothing, Lucas is your synthesist, and Captain is the stressed planner trying to keep them in line.
* **The Swarm Agreement (Brutal Pre-Work Negotiation)**: Before they write a single line of code, Captain has to pitch a contract of work criteria. The Evaluator (a digital lie detector) continuously slams the door in Captain's face screaming *"TOO GENERIC! REVISE!"* using real semantic scoring until the contract is mathematically precise. Only then do they sign the contract in digital blood (Ed25519 signatures) and start working.
* **Simulated Heart Attacks & Recovery (`.ktrans` + Blackboard)**: Suppose Benjamin has a catastrophic crash (which we simulate for testing!) and exits the process with a fatal code `101`. Korg doesn't cry. It immediately scans the flight recorder (`.ktrans` journal), recovers Benjamin's partial changes, safely updates the shared state (Blackboard), spawns a clone of Benjamin, hands him the restored state, and commands him to keep building. Zero state is lost.
* **Post-Campaign Factual Purge (Yvaeh Mode)**: Once the campaign finishes, Korg launches a self-compounding brain sweep. It scans your vault, finds notes that contradict each other (e.g. Note A says "deploy to AWS", Note B says "deploy to GCP"), screams *"CONTRADICTION DETECTED!"*, resolves the conflict using semantic similarities, and creates beautiful, interconnected conceptual notes with backlinks.

---

## 🎯 Real-World Use Cases (Technical vs. Chaotic)

### 1. Autonomous Software Engineering Swarms
* **Technical Spec**: Parallelizes work package generation (`RouteWork`) across specialized personas, enforcing state locking (`WRITE`, `READ`, `IDLE`) on a shared memory blackboard to prevent branch collision, while auditing structural AST mutations.
* **Chaotic Layman Version**: Enforces a strict code-writing assembly line. No more "whoops, I overwrote your file". If Lucas is writing a patch, he holds an exclusive red lock (`🔒 WRITE`), and Benjamin is forced to sit in the corner (`IDLE`) until Lucas releases the lock. The Evaluator validates the diff, and if the code is garbage, it rolls back the workspace instantly.

### 2. Deep Intelligence & Adversarial Fact Synthesis
* **Technical Spec**: Aggregates multi-source information inputs using a multi-round competitive Arena. Compares semantic drift using 24-bit cosine similarity matrices, flagging epistemic entropy spikes to detect hallucinations.
* **Chaotic Layman Version**: We put four AI agents into a competitive cage match. They all try to synthesize research, and the Evaluator grades them against 5 brutal rubrics. If an agent tries to lie or make up a facts, its "Semantic Entropy" spikes, the Evaluator flags it as "low confidence," and the Leader rejects their output, forcing them to re-evaluate their entire life.

### 3. Critical Production Diagnostics & Self-Healing Operations
* **Technical Spec**: Real-time streaming of cryptographically sealed, framed ACP transactions over standard output, supporting hot-swappable recovery loops and human manual overrides.
* **Chaotic Layman Version**: A background system monitor that detects infrastructure issues, designs a cure, validates the plan, and asks a human operator for approval with a giant neon blinker. If the system is in headless mode, it outputs beautifully structured diagnostic boxes directly to logs.

---

## ✨ System Features

* **Immersive Ratatui TUI** — A focused, real-time operator dashboard with custom 24-bit RGB TrueColor palette overrides, stacked telemetry gauges (Semantic Entropy, Swarm Risk, Doom Probability), blackboard lock grids, and scroll panels.
* **Smart CLI Onboarding Welcome** — When executed with zero arguments, `korg` prints a stunning cyberpunk ASCII banner and quick-start guide, defaulting campaigns to interactive TUI mode unless `--headless` is explicitly passed.
* **ACP v1.17 Native** — Full support for signed `MessageEnvelope`s, JCS canonicalization (RFC 8785), and Ed25519 signatures on every artifact.
* **5 Harsh Combinatorial Rubrics** — Independent adversarial evaluation across Trajectory Efficiency, Epistemic Integrity, Tool-Use Precision, Semantic Adherence, and Resource Utilization.
* **Signed `.ktrans` + Compaction** — Tamper-evident transactional memory. Every meaningful change is recorded in cryptographically signed artifacts. Automatic compaction + fast base-snapshot recovery.
* **Yvaeh Factual Reconciliation & Concept Synthesis** — Autonomous scan of vaults to isolate and resolve conflicting facts and build associative backlink webs automatically.

---

## 📦 How to Setup & Package (Make It Work Instantly)

`korg` supports multiple packaging methods, making it extremely easy to distribute to teammates or run in production.

### Method 1: Global Cargo Installation (The Developer Way)
Ensure you have Rust and Cargo installed, then compile in high-performance release mode and install it globally:
```bash
git clone https://github.com/New1Direction/korg
cd korg/reference-implementations/rust/grok-acp-harness
cargo install --path . --force
```
This registers the binary globally. You can now type `korg` from **any** directory on your system.

### Method 2: Portable Container (The Sandbox Way)
Build and run `korg` inside a secure, statically compiled Docker container. Perfect for servers or teams without Rust:
```bash
# Build the container
docker build -t korg .

# Run the cyberpunk welcome banner
docker run --rm -it korg

# Run a headless campaign inside the container
docker run --rm -it korg --headless "Analyze workspace credentials and establish key rotations"
```

### Method 3: Statically Compiled Binary (The Release Way)
We've included an automated GitHub Actions release workflow at `.github/workflows/release.yml`. When you push a git tag (e.g. `v0.1.0`), it automatically compiles and uploads static binaries for:
* **Linux**: `korg-linux-x86_64` (statically linked via MUSL)
* **macOS Intel**: `korg-macos-x86_64`
* **macOS Apple Silicon**: `korg-macos-arm64`
* **Windows**: `korg-windows-x86_64.exe`

### Method 4: The Custom Homebrew Tap (The Apple Way)
We have provided a formula template at `packaging/korg.rb`. You can distribute `korg` through your team's custom Homebrew tap so users can run:
```bash
brew tap your-org/tap
brew install korg
```

---

## 🚀 How to Use (Commands Reference)

### 1. Launch the Immersive Ratatui Dashboard (Recommended)
Watch the swarm reason, negotiate contracts, lock the state, and evaluate metrics in real time:
```bash
korg tui
```

### 2. Run a Headless Swarm Campaign
Execute campaigns inside scripts, automated build servers, or continuous integration lines:
```bash
korg --headless "Verify premium TUI campaign"
```

### 3. Run the Leader Swarm Benchmarks (Demo Mode)
Watch Korg simulate worker panics, apply `.ktrans` transactional recoveries, and dynamically scale the agent pool up to 16 agents:
```bash
korg leader --demo
```

### 4. Replay a Prior Campaign with Cryptographic Verification
Verify the Ed25519 signatures of a past run to prove no quiet tampering took place:
```bash
korg leader --replay latest
```

---

## 📊 Telemetry Output Examples (Headless Mode)

When executing in `--headless` mode, Korg prints clean visual separators (`───`), unicode marks (`✓`, `✗`, `⚡`, `⧖`, `💾`), and a perfect double-bordered verdict summary box:
```
╔════════════════════════════════════════════════════════════════════╗
║           HEAVY-TIER EVALUATOR VERDICT SUMMARY                     ║
╠════════════════════════════════════════════════════════════════════╣
║ Session: 019e49ec-42a6-7793-a9cf-6d685c718ea5                      ║
║ Task:    Verify premium TUI campaign                               ║
╠════════════════════════════════════════════════════════════════════╣
║ Overall Verdict     : PASS                                         ║
║ Rubrics Passed      : 5/5 (all clear)                              ║
║ Semantic Entropy    : 0.163  (threshold ~0.78)                     ║
║ Recommended Action  : SCALE_UP                                     ║
║ Doom Loop Detected  : FALSE                                        ║
║ Productive Death    : FALSE                                        ║
╚════════════════════════════════════════════════════════════════════╝
```

---

## 🛠️ Deep Technical Architecture

Korg is built on three tightly integrated layers forming a closed feedback loop:

```
                  ┌──────────────────────┐
                  │   Leader & Broker    │◄─── Human Approval Gates
                  └──────────┬───────────┘
                             │
                  RouteWork  │  Ingest Pulses
                             ▼
  ┌─────────────────────────────────────────────────────┐
  │                 Swarm Workers                       │
  │  [Captain]     [Harper]     [Benjamin]     [Lucas]  │
  └──────────────────────────┬──────────────────────────┘
                             │
                             ▼  TraceEvents
                  ┌──────────────────────┐
                  │ 5-Rubric Evaluator   │
                  └──────────────────────┘
```

1. **Workers (Personas)**: Specialized concurrent subprocess agents (Captain, Harper, Benjamin, Lucas) communicating via signed ACP messages and executing within isolated worktrees.
2. **Leader + Broker**: Manages work package routing, contract criteria negotiation, dynamic swarm scaling (from 4 up to 16), and signed `.ktrans` recovery from crashed workers.
3. **Adversarial Evaluator**: Validates trajectories using combinatorial rubrics, calculates semantic entropy, and safeguards progress.

---

## 🛠️ Development & Testing

```bash
# Verify the build compiles correctly
cargo check

# Run all 12 automated unit and integration tests
cargo test

# Auto-format and lint code
cargo fmt
cargo clippy --fix
```

---

## 📄 License

MIT License © 2026

*Minimal. Technical. Serious. (And incredibly chaotic).*