# Korg User Guide & Cockpit Manual

Hello, Operator! Welcome to Korg, your command center for orchestrating autonomous multi-agent software engineering swarms.

Korg isn't just another AI tool; it's a complete, self-contained development environment where a team of specialized AI agents—**Captain** (the architect), **Harper** (the researcher), **Benjamin** (the coder), and **Lucas** (the tester)—collaborate in real-time. They operate around a shared **Blackboard**, a highly observable state that ensures every agent has perfect context.

Every action, from planning to code synthesis, is cryptographically signed and recorded on a **Merkle-DAG ledger**, creating a tamper-proof audit trail of the AI's entire thought process. When the swarm needs guidance or hits a security boundary, Korg's revolutionary **zero-overlap cockpit** opens an inline gateway, allowing you to intervene without ever losing sight of critical telemetry.

This guide will walk you through launching your first campaign and mastering the Korg cockpit. Let's begin!

---

## 🚀 Quick Start: Launching Your First Campaign

Getting started with Korg is incredibly simple. First, ensure all components are correctly compiled.

### 1. Verify Installation

Run a quick check to confirm your Korg installation is healthy:

```bash
cargo check
```

### 2. Launch the Web Cockpit

To start a new campaign, use the `korg campaign` command. The `--web` flag launches the real-time browser cockpit, and the `--prompt` flag gives the swarm its mission.

```bash
cargo run -- campaign --web --prompt "Refactor the authentication layer to use memory-mapped vectors"
```

Korg will automatically spin up its Axum web server and open the cockpit in your default browser at `http://localhost:8080`.

---

## 🗺️ Tutorial: A Guided Tour from Prompt to Validation

Let's walk through a complete campaign, from issuing the initial prompt to validating the final, merged code.

### Step 1: The Cockpit Comes Alive

Once you run the launch command, the Korg cockpit springs to life. You're now looking at the command center for your AI swarm. The interface is a **6-pane grid** designed for maximum information density and zero visual clutter.

### Step 2: The Swarm Begins its Work

The campaign starts immediately. Here’s what to watch:

-   **Console (Bottom-Left):** You'll see a stream of `stdout` logs as the swarm begins its task. Captain formulates a plan, and agents begin executing shell commands in their secure sandbox.
-   **Active Workspace (Top-Left):** As Benjamin (the coder) starts writing code, you'll see the files appear here, complete with live diffs (`+` for additions, `-` for deletions).
-   **Telemetry (Top-Right):** The four metric cards and the entropy sparkline will begin to update, showing the swarm's `Velocity`, `Risk`, `Progress`, and cognitive `Entropy`.
-   **Timeline (Middle-Right):** Nodes representing transactions (`tx_00`, `tx_01`, etc.) will appear on the Merkle-DAG, building the cryptographic history of the campaign.

### Step 3: The Zero-Overlap Human Gateway

Korg is designed for human-in-the-loop collaboration. When the swarm needs your approval for a plan or hits a security policy, it doesn't interrupt you with a jarring popup.

Instead, the **Inline Actions Pane** smoothly slides open at the top of the left column, pushing the other panes down.