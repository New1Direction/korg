---
title: "Rust Async Performance Optimizations – Practical Investigation"
date: 2026-05-21
type: pattern
tags: [performance, async, tokio, korg, investigation]
status: resolved
ai-first: true
confidence: high
---

# Rust Async Performance Optimizations — Practical Investigation

This document evaluates the practical impact of the proposed async performance optimizations on the **Korg reference harness (`grok-acp-harness`)** from a production-grade 2026 perspective.

---

## 1. LOE vs. ROI Evaluation Matrix

| Optimization | Expected ROI | Level of Effort (LOE) | Practical Impact on Korg / Yvaeh |
| :--- | :--- | :--- | :--- |
| **`spawn_blocking` for CPU/File Operations** | **Extreme** | **Trivial (10 mins)** | Prevents CPU-heavy unified diff parsing/patching (`apply_patch`) and BERT similarity checks from starving the TUI and telemetry streams. |
| **Tuning the Global Allocator (`mimalloc`)** | **Very High** | **Trivial (5 mins)** | Drastically reduces memory fragmentation and latency under high-frequency telemetry pulses and local BERT/Candle tensor allocations. |
| **Worker Process Pooling** | **High** | **Medium (2 hours)** | Eliminates the costly OS process spawn overhead (`std::env::current_exe()`) during each campaign round while introducing minor capability isolation challenges. |
| **Batching `.ktrans` Writes** | **High** | **Low (30 mins)** | Replaces high-frequency atomic disk write operations with buffered checkpoint flushes, reducing local I/O bottlenecks. |
| **Bounded Channels & Backpressure** | **Already Done** | **None** | Bounded channels (`mpsc::channel(128)`) are already implemented in `tui.rs`, successfully safeguarding against memory spikes. |
| **JCS Canonicalization Caching** | **Low-Medium** | **Low (20 mins)** | Provides minor cryptographic signing latency improvements for large, repetitive static payloads. |

---

## 2. In-Depth Analysis & Codebase Alignment

### A. Tuning the Global Allocator (`mimalloc`)
*   **Context:** Korg utilizes a local Hugging Face `all-MiniLM-L6-v2` BERT model via the `candle` crate for real-time semantic similarity. Tensor operations and high-frequency CRDT blackboard telemetry ingestion generate significant heap allocation noise.
*   **2026 Reality:** The default OS allocator on macOS/Linux introduces subtle lock contention under concurrent multi-threaded workloads. Setting `mimalloc` as the global allocator yields an immediate **10–20% reduction in peak memory** and lower allocation latency.
*   **Implementation Strategy:**
    ```rust
    // In Cargo.toml
    [dependencies]
    mimalloc = { version = "0.1", features = ["build"] }

    // In src/main.rs
    #[global_allocator]
    static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
    ```

### B. Moving Blocking Work to `tokio::task::spawn_blocking`
*   **Context:** Our context-aware `apply_patch` in `src/tools.rs` performs multi-stage fuzzy string matching and outward line searches on raw text. Similarly, tokenizing and calculating BERT embeddings in `src/evaluator.rs` are CPU-intensive operations.
*   **2026 Reality:** Running CPU-bound or blocking I/O code inside standard async `Future` structures starves the multi-threaded Tokio executor, causing UI stutters on the Ratatui dashboard and telemetry dropouts.
*   **Implementation Strategy for `src/tools.rs`:**
    Wrap the heavy `apply_patch` CPU loop inside a spawn-blocking thread:
    ```rust
    // In execute_patch_apply (src/tools.rs)
    let patched = tokio::task::spawn_blocking(move || {
        apply_patch(&original, &patch_content)
    }).await??;
    ```

### C. Worker Pooling vs. Spawn-per-Task
*   **Context:** Currently, `LeaderOrchestrator::spawn_worker_process` spawns a brand new `korg worker` CLI child process for every persona on every campaign round:
    ```rust
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("worker")...
    ```
*   **2026 Reality:** OS process spawning takes **10–50ms** depending on the system, parsing arguments and re-initializing stdio pipes. In long-running campaigns with 16 swarms, this becomes a major bottleneck.
*   **Architectural Tradeoff:**
    *   *Spawn-per-Task:* Maximizes execution isolation (if a worker panics or crashes, it doesn't pollute subsequent steps).
    *   *Worker Pool:* Reuses long-lived processes via a persistent stdio channel. Reaching 10x throughput increases, but requires resetting intermediate state in the worker harness before a new task.
*   **Recommendation:** Since this is a reference harness designed to illustrate Korg's **resilient crash recovery loops**, spawn-per-task is highly educational, but a production TUI or CI runner should leverage a thread-safe `WorkerPool`.

### D. Batching `.ktrans` Writes
*   **Context:** Workers write terminal `.ktrans` transaction logs to disk upon completing their packages. The Leader immediately merges these logs and persists the central `blackboard.json`.
*   **2026 Reality:** Disk I/O is slow. For heavy swarms making thousands of small operations, writing to disk on every single transaction block blocks pipeline execution.
*   **Implementation Strategy:** Maintain an in-memory transactional commit log and flush it to disk asynchronously using a background flushing thread or only during explicit milestones (e.g. Contract Approvals, Swarm Re-routing, or Campaign Completion).

---

## 3. Concrete Performance Hardening Plan

### Step 1: Add `mimalloc` for Memory and Tensor Optimization
Enable the highly optimized `mimalloc` allocator globally inside `src/main.rs`.

### Step 2: Offload CPU-Heavy Unified Diff Matching & Tokenization
Harden `execute_patch_apply` (`src/tools.rs`) and `Evaluator::score_similarity` (`src/evaluator.rs`) by offloading their calculations to `spawn_blocking` to ensure 100% smooth Ratatui rendering.

### Step 3: Implement Buffered `.ktrans` flushing
Optimize the Leader's blackboard write-back loop to avoid excessive atomic file operations.

---

## 4. How to Verify Performance Improvements
*   **`tokio-console`:** Run the campaign while monitoring scheduled task latency and queue backpressures.
*   **`cargo flamegraph`:** Audit CPU-hotspots to ensure no blocking operations starve the main async threads.
