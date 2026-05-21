---
date: 2026-05-20
type: reference-architecture
tags: [reference-architecture, serving, kv-cache, paged-attention, prefix-caching, cluster-engineering]
harness: korg
domain: acp, orchestration, resource-management
status: active
ai-first: true
---

# Serving Infrastructure and KV Cache Lifecycle

This note extracts the low-level serving and memory management techniques that enable the Grok 4.20 Heavy 16-agent swarm to run with true parallel, independent runtimes on shared hardware.

See the ground-truth architecture in [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] for full context.

---

## Massive MoE Model Serving at Scale

The base model is a ~3T-parameter Mixture-of-Experts (MoE) architecture. The following techniques work together to support up to 16 concurrent independent inference runtimes:

### Paged KV Cache
- KV tensors are broken into fixed-size pages (typically 16–32 tokens per page).
- Each runtime maintains its own virtual page table.
- Pages are allocated on-demand and can be evicted independently.
- This allows 16 agents to have effectively private contexts while sharing physical GPU memory.
- Page table updates are atomic and logged for provenance.

### Continuous Batching + Dynamic Scheduling
- The engine continuously batches new tokens from all active agents into the same forward pass.
- The scheduler reorders and reprioritizes requests every few milliseconds based on urgency, confidence, and remaining timeout.
- This keeps GPU utilization high even with many independent decoding loops.

### Prefix Caching / Shared Context
- Common prefix tokens (shared prompt segments or blackboard state) are stored once in a reference-counted prefix cache.
- Each agent’s KV cache only stores its unique suffix.
- When an agent writes to the blackboard, the prefix cache is updated atomically so all runtimes see the change with minimal memory duplication.

### Expert Routing Under High Concurrency
- Per-token routing uses a lightweight router network.
- Under heavy load the system uses:
  - Grouped routing (multiple tokens share the same expert assignment batch)
  - Expert buffering (temporary queues per expert)
  - Asynchronous expert updates so routing tables stay consistent without blocking decode

---

## Multi-Session / Multi-Runtime Isolation

Each of the 16 agents is a fully independent inference runtime/session:

- Separate KV cache / attention state (own virtual page table)
- Separate generation/decoding streams
- Isolated execution environments (especially visible in Grok Build via per-agent git worktrees)
- Independent state & persona (own system prompt + RL policy)
- Separate process/session model (distinct child processes or virtual sessions managed by the Leader/Broker)

This isolation guarantees that one agent’s context corruption, infinite loop, or memory spike cannot affect the others, while still allowing efficient sharing of model weights and prefix cache.

---

## KV Cache Lifecycle

Maintaining massive context active states across many sub-agents is extremely memory-intensive. The infrastructure uses a strict hydration/dehydration lifecycle:

- **Active Eviction** — The moment an agent finishes its primary debate loop and goes idle, its active context states are chunked, compressed, and flushed out of active High Bandwidth Memory (HBM3e).
- **Cold Tiering** — These states are offloaded into lightning-fast NVMe/flash storage tiers on the host racks.
- **On-Demand / Predictive Hydration** — When the orchestration layer knows a sub-agent will soon need a historical context block, it proactively initiates high-speed storage fabric transfers (DMA-style) to pull the fragments back into live memory before the agent’s execution step.

Under memory pressure the engine evicts pages using a recency × importance heuristic and can recompute or swap to host memory if needed.

---

## Topology-Aware Scheduling (Zero-Hop Locality)

To keep high-frequency inter-agent communication viable during debate loops, the resource allocator prefers co-locating related sub-agents:

- Related agents are bound to the same physical server rack or adjacent blocks.
- High-speed interconnects (InfiniBand, NVLink, Spectrum-X fabrics) provide near-zero-hop latency.
- This turns what would be a distributed network bottleneck into an ultra-fast local backplane exchange.

---

## Why This Matters for Korg

These serving and memory techniques are the concrete foundation that makes the entire Leader-Broker-ACP model and the 16-agent swarm practical at cluster scale. Any reference harness or alternative implementation that wants to achieve similar scale and isolation must understand and respect these mechanisms (or provide equivalent primitives).

They also explain many of the performance, cost, and fairness characteristics observed in production Heavy usage.

---

## Related

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground truth architecture containing the original descriptions.
- [[wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md]] — Complementary note on the dynamic cost accounting and throttling that works together with these serving techniques.
- [[wiki/mechanisms/isolation-routing.md]] and [[wiki/mechanisms/transactional-memory.md]] — The Korg primitives (worktree isolation and transactional blackboard) that must coexist with these cluster-level memory and scheduling realities.