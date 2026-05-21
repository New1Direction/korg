# Korg User Guide & Cockpit Manual

Welcome, Operator!

You're about to step into the command center for Korg, our autonomous multi-agent engineering swarm. This guide will walk you through everything from launching your first campaign to performing advanced interventions like security overrides and timeline forks.

Korg isn't just another AI tool—it's a complete, observable, and secure development environment where specialized AI agents collaborate on your codebase. Let's get you started.

---

## 🚀 Quick Start: Launching the Cockpit

Getting Korg up and running is simple. From your project's root directory, you can launch a campaign in two primary modes: the terminal-based TUI or the rich web-based Cockpit.

### 1. Verify Your Setup
First, run a quick check to ensure all components are compiled and ready.
```bash
cargo check
```

### 2. Launch the Web Cockpit
To start a new campaign and view it in our stunning visual cockpit, use the `--web` flag. Your browser will automatically open to `http://localhost:8080`.

```bash
# Example: Launch a web campaign to refactor the auth system
cargo run -- campaign --web --prompt "Refactor the authentication layer to use JWTs"
```
Your terminal will confirm the server is running:
```
[korg] Axum server listening on http://localhost:8080
[Web] Automatically opening browser at: http://localhost:8080
```

---

## 🗺️ Tutorial: Your First Campaign from Prompt to Validation

Let's walk through a complete campaign to add a new feature. Our goal: **"Add a rate-limiter to the public API endpoints."**

### Step 1: The Prompt & Launch
Initiate the campaign with your goal. Korg's **Captain** agent will immediately begin decomposing this high-level task into a concrete execution plan.

```bash
cargo run -- campaign --web --prompt "Add a rate-limiter to the public API endpoints"
```

### Step 2: Observing the Cockpit
Your browser opens to the Korg Cockpit, a real-time window into the swarm's mind. The interface is a **zero-overlap 6-pane grid**, ensuring you never lose sight of critical information.

![Korg Cockpit Layout](httpst://raw.githubusercontent.com/grok-ai/grok-1/main/assets/architecture.png)
*(Conceptual layout inspired by Grok.com's design language)*

Here’s what you’re seeing:

| Pane Location | Pane Name | What It Shows |
| :--- | :--- | :--- |
| **Top-Left** | **Swarm Plan & Actions** | The swarm's current plan and the **Zero-Overlap Workspace** for human interaction. |
| **Mid-Left** | **Active Workspace** | A live view of the code files being actively edited by the swarm agents. |
| **Bottom-Left** | **Console Stream** | The raw, unfiltered `stdout` from the swarm's build and test commands. |
| **Top-Right** | **Telemetry** | Real-time sparkline charts for system entropy, risk, and swarm velocity. |
| **Mid-Right** | **Timeline & Vision** | The interactive **Merkle-DAG** transaction graph and a carousel of screenshots from the swarm's work. |
| **Bottom-Right**| **Provenance** | Cryptographic signatures, agent lock status, and the state of the shared **Blackboard**. |

### Step 3: The Swarm at Work
You'll see the panes come alive:
- **Console Stream:** `cargo check` and `cargo test` commands fly by as **Benjamin** (the coder) and **Lucas** (the tester) validate changes.
- **Active Workspace:** Code diffs appear in real-time, showing new lines in green and deletions in red.
- **Timeline:** New transaction nodes (`tx_01`, `tx_02`...) appear on the Merkle-DAG, each representing a cryptographically signed state change.
- **Telemetry:** The entropy sparkline fluctuates as the swarm explores different solutions.

### Step 4: Human-in-the-Loop Approval
Suddenly, the **Swarm Plan** pane smoothly expands downwards, revealing an **Emerald Consensus Gate**. This is Korg's **Zero-Overlap UI** in action—no popups block your view of the other panes.

The swarm is asking for your approval on a set of acceptance criteria before proceeding.
> **[SWARM CONTRACT]**
> The swarm proposes the following contract:
> 1. Implement `leaky-bucket` algorithm for rate limiting.
> 2. Add new tests for `429 Too Many Requests` responses.
> 3. Ensure no more than 100 requests per minute per IP.

You can type `Y` to approve or `N` to demand a revision, all while monitoring the live console and telemetry.

### Step 5: Validation and Completion
Once you approve, the swarm executes the final implementation and testing. The progress bar in the Telemetry pane reaches 100%, and the final transaction is committed to the ledger. The campaign is complete, and the new rate-limiter is securely merged into your codebase.

---

## 🌌 The Cockpit: A Deep Dive

Korg's cockpit is more than a dashboard; it's an interactive command center powered by a Rust-based Axum web server and real-time Server-Sent Events (SSE).

### The 6-Pane Grid Explained

1.  **[Top-Left] Swarm Plan & Zero-Overlap Workspace**
    This is your primary interaction point. When the swarm needs you, one of three drawers will slide open without covering any other part of the UI:
    -   🟡 **Amber Security Gate:** Flashes when a security policy is triggered. Requires your approval (`Y`/`N`) for sensitive operations.
    -   🟢 **Emerald Consensus Gate:** Appears when the swarm needs you to approve a plan or contract.
    -   🔵 **Cyan Steering Fork:** Opens when you manually trigger a timeline fork (`F`) to give the swarm a new directive.

2.  **[Mid-Left] Active Workspace Code**
    See exactly what the AI is writing. This pane shows a live, color-coded diff of the file currently being modified by an agent like **Benjamin**. You can watch the code evolve in real-time.

3.  **[Bottom-Left] Stdout Console Stream**
    The ground truth. This pane streams the raw, unfiltered terminal output from the swarm's sandbox. Watch `cargo` builds, test suites, and tool outputs as they happen.

4.  **[Top-Right] Real-Time Telemetry**
    This is the swarm's EKG.
    -   **Metrics:** Key performance indicators like `Velocity` (actions per second), `Risk` (potential for negative outcomes), and `Progress` (towards the goal).
    -   **Entropy Sparkline:** Visualizes the "semantic entropy" of the swarm's thinking. High, chaotic entropy might indicate the swarm is confused, while low, stable entropy suggests focused progress.

5.  **[Mid-Right] Timeline & Vision Scrubber**
    Your time machine and security camera.
    -   **Merkle-DAG Graph:** A visual representation of the swarm's entire history. Each node is a cryptographically signed transaction. You can click any node to view its state.
    -   **Vision Scrubber:** A screenshot carousel showing what the swarm sees. Korg's vision policy engine scans these frames for sensitive data leaks in real-time.

6.  **[Bottom-Right] Provenance & Blackboard**
    The cryptographic heart of Korg.
    -   **Blackboard:** A view into the shared state that all agents "orbit." It contains the current plan, results, and swarm status.
    -   **Agent Locks:** See which agent (e.g., `Captain`, `Harper`) has a `read` or `write` lock on the blackboard.
    -   **Signatures:** View the `ed25519` signature and Merkle root for the current transaction, guaranteeing a tamper-proof audit trail.

### Keyboard Shortcuts

Master the cockpit with these essential keyboard shortcuts.

| Key | Action | Description |
| :--- | :--- | :--- |
| `q` | **Quit** | (TUI Mode) Exits the Korg terminal interface. |
| `←` / `→` | **Time-Travel Scrub** | Moves the playhead backward or forward along the Merkle-DAG timeline. |
| `F` | **Steering Fork** | Opens the Cyan Steering Fork drawer to create a new branch of execution from the current playhead position. |
| `Y` / `N` | **Manual Override** | Approves (`Yes`) or rejects (`No`) a prompt from a Security or Consensus Gate. |

---

## 🎬 Actionable Scenarios

See how these features come together in real-world situations.

### Scenario A: Zero-Trust Security Intercept

Benjamin, our coder agent, is debugging an API integration. It runs a `curl` command to test an endpoint, but the output accidentally includes a live `API_KEY`.

1.  **Detection:** Korg's **Vision Policy Engine** is constantly taking screenshots of the workspace. It performs OCR on the latest frame and its pattern-matching rules immediately flag the string `API_KEY=...`.
2.  **Redaction:** The raw screenshot is instantly redacted in memory. The version displayed in the **Vision Scrubber** carousel is replaced with a blurred or blacked-out image (`BLACKOUT_PNG_BASE64`).
3.  **Intervention:** The **Amber Security Gate** flashes in the Zero-Overlap Workspace. A message appears:
    > **[SECURITY POLICY INTERCEPT]**
    > A potential secret (`API_KEY`) was detected in the terminal output. The action has been blocked and the visual evidence redacted.
    >
    > **Approve redacted broadcast? (Y/N)**
4.  **Resolution:** The operator presses `N`. The swarm terminates the faulty command and logs the security event to the provenance ledger, preventing a catastrophic secret leak.

### Scenario B: Playhead Steering Fork

The swarm is tasked with optimizing a database query. It begins implementing a complex but inefficient join strategy. The operator notices the `Entropy` sparkline rising and the `Progress` metric stalling.

1.  **Time-Travel:** The operator uses the `←` Arrow Key to scrub the playhead on the **Timeline** back to `tx_03`, the point just before the swarm committed to the bad strategy.
2.  **Fork:** The operator presses `F`. The **Cyan Steering Fork** drawer slides open.
3.  **New Directive:** The operator types a new, more precise instruction into the input field:
    > `Rewrite the db layer using memory-mapped vectors and a pre-computed index.`
4.  **Execution:** The operator hits Enter. Korg's `LeaderOrchestrator` performs a `handle_operator_fork`:
    - It physically resets the codebase back to the state at `tx_03`.
    - It logically rehydrates the swarm's shared **Blackboard** to match that point in time.
    - It injects the new directive.
5.  **Resolution:** A new branch appears on the Merkle-DAG, originating from `tx_03`. The swarm, now armed with better instructions, proceeds down this new, more efficient path. The `Entropy` metric stabilizes, and `Progress` begins to climb again.