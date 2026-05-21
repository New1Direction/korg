# 🌌 Korg — Swarm Orchestration & Zero-Trust State Ledger

> **The Command Center for Autonomous Multi-Agent Swarms.** 
> Built with zero-overlap inline workspaces, a localized Merkle Directed Acyclic Graph (DAG) state ledger, and real-time human security gateways.

---

## ⚡ The Core Manifesto

Traditional AI workflows are linear, closed, and fragile. **Korg** changes the paradigm. 

Korg operates on a **Blackboard Architecture** modeled as a **Gravitational Well**. Instead of agents passing messages in blind pipelines, a specialized multi-agent swarm (Captain, Harper, Benjamin, Lucas) orbits a shared, highly observable state blackboard. They concurrently propose code updates, run test validations, and debate execution contracts. 

Every single transaction is physically recorded onto a **Merkle Directed Acyclic Graph (DAG)** blockchain, creating a cryptographically verifiable ledger of AI reasoning. When critical security policies are breached or consensus fails, Korg intercepts the swarm, opening a **zero-overlap inline gateway** that allows human operators to inject criteria or physically fork the codebase back to any transaction block.

---

## 🏗️ Architecture & High-Tech Design Language

Korg’s layout and style are built for maximum developer focus, heavily inspired by **Grok.com** and **xAI**.

```
    ┌────────────────────────────────────────────────────────┐
    │                     OPERATOR CONSOLE                   │
    ├───────────────────────────┬────────────────────────────┤
    │  [LEFT COLUMN]            │  [RIGHT COLUMN]            │
    │                           │                            │
    │  1. Swarm Plan & Status   │  1. Live SVG Merkle DAG    │
    │                           │     Transaction Graph      │
    │  2. ZERO-OVERLAP WORKSPACE │                            │
    │     ├─ Amber Security Gate│  2. Real-Time Telemetry    │
    │     ├─ Emerald Consensus  │     Sparklines & Entropy   │
    │     └─ Cyan Steering Fork │                            │
    │                           │  3. Stdout Console Stream  │
    │  3. Active Workspace Code │     (Live Raw Log Feed)    │
    └───────────────────────────┴────────────────────────────┘
```

### 1. The Zero-Overlap Workspace
Traditional modal overlays are banned in Korg. Full-screen popups and blur backdrops block critical data streams. Korg uses an **inline document-flow layout**:
- When a **Human Security Gate** or **Swarm Consensus Negotiation** triggers, a custom actions drawer smoothly slides open inside the left column, pushing content down.
- The developer can monitor the **real-time stdout log feed** and **sparkline telemetry charts** in the right column *while* typing an approval override or custom acceptance criterion.

### 2. The Merkle-DAG Blockchain
- Each transaction (`tx_00` to `tx_0N`) represents a logically validated state change of the repository.
- Contains the parent hashes, state Merkle root, and codebase Merkle root.
- If a swarm run deviates into unstable code territory, the operator can click any node in the Merkle graph to perform a **Playhead Steering Fork**, physically resetting the git tree and logical blackboard back to that genesis signature.

---

## 🚀 Quick Start & CLI Reference

### 1. Verification of Code Integrity
Ensure all Rust components, Axum web handlers, and security deciders are compiled and valid:
```bash
cargo check
```

### 2. Launch the Swarm Web Cockpit
Launch the local Axum cockpit console connected to the active campaign telemetry:
```bash
cargo run -- campaign --web
```
Once active, navigate your browser to: **`http://localhost:8080`**

### 3. Record Live UI Keyframes
Generate high-fidelity, dual-density 4K screenshots of the active dashboard and expanded zero-overlap panels:
```bash
node scratch/record_cockpit.js
```

---

## 🎙️ NotebookLM Podcast Explainer Blueprint
*Use the questions and narratives below to prompt NotebookLM into generating a high-tempo, non-monotone, highly conversational Deep Dive podcast.*

### Q: Why does Korg use a "Blackboard" instead of standard AI chains?
> **Answer for NotebookLM to read:**
> Standard AI chains are like a game of telephone—if Agent A makes a small mistake, it compounds down the line until the final output is completely broken. Korg flips this. The blackboard acts like a giant physical chalkboard in the center of a room. All agents stand around it, looking at the exact same codebase, logs, and Merkle tree. They work concurrently, correcting each other's code in real-time, preventing errors from propagating.

### Q: What is the "Zero-Overlap UI" and why does it matter to developers?
> **Answer for NotebookLM to read:**
> Imagine you are a pilot flying a high-performance jet, and suddenly an emergency alert pops up and physically covers your entire windshield. That is what traditional modal popups do in software. Korg's zero-overlap UI ensures your visual "windshield" is never blocked. The action drawers slide open inside the layout flow. You can keep your eyes locked on the live-streaming console logs, system entropy charts, and Merkle graphs while you authorize or reject swarm actions.

### Q: How do "Playhead Steering Forks" prevent AI swarms from hallucinating?
> **Answer for NotebookLM to read:**
> When an autonomous swarm starts editing code, it can easily run down a rabbit hole of bad assumptions. With Korg, you don't have to restart the run or delete your work. The Merkle graph acts as a time-machine. You can physically grab the timeline slider (the playhead), scrub back to `tx_02` before the swarm got confused, click "Execute Fork", type a new steering directive like *"focus on robust parser rules"*, and watch the swarm branches fork into a new, safe codebase direction.
