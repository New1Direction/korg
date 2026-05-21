---
date: 2026-05-20
type: reference-architecture
tags: [reference-architecture, token-bucket, resource-gating, throttling, multi-agent, cluster-engineering]
harness: korg
domain: acp, orchestration, resource-management
status: active
ai-first: true
---

# Token-Bucket Throttling and Resource Gating

This note extracts the kernel-level resource gating model used by the Grok 4.20 Heavy multi-agent runtime. It explains how the system protects cluster health while supporting up to 16 concurrent independent agents with massive context.

See the ground-truth architecture in [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] for full context.

---

## Overview

SuperGrok Heavy sessions can consume dramatically more compute than standard inference because of concurrent agents, large KV caches, and high token velocity. To prevent any single session from starving the cluster, the gateway uses a sophisticated **dynamic token-bucket** that tracks *effective compute burn* rather than raw request rate.

---

## Effective Burn Rate Formula

The gateway calculates an **EffectiveBurnRate** for every major inference batch and every new sub-agent spawn:

\[
\text{EffectiveBurnRate} = (T_{\text{in}} + T_{\text{out}}) \times \bigl(1 + (N_{\text{agents}} - 1) \times \alpha\bigr) \times \bigl(1 + \bigl(\frac{C}{C_{\max}}\bigr)^2 \times \beta\bigr) \times \gamma
\]

**Variables**
- \( T_{\text{in}} + T_{\text{out}} \): Base tokens consumed
- \( N_{\text{agents}} \): Number of active sub-agents (0–16)
- \( \alpha \approx 0.6 \): Concurrency multiplier (captures non-linear cost of additional parallel runtimes)
- \( C \): Current context tokens in use (up to ~2M)
- \( \beta \approx 1.6 \): Context depth penalty (quadratic because KV cache memory grows super-linearly)
- \( \gamma \): KV pressure factor (1.0–2.0), raised when cluster-wide HBM utilization is high

---

## Bucket Logic and Rolling Window

The bucket itself is a **rolling multi-hour window** (typically 4–12 hour effective waves, tuned per region):

\[
B(t) = B_{\max} - \int_{t-W}^{t} \text{EffectiveBurnRate}(\tau) \, d\tau + R(t)
\]

- \( B_{\max} \): Session budget (significantly higher for Heavy tier)
- \( W \): Rolling window duration
- \( R(t) \): Refill rate (higher for Heavy tier)

---

## Throttling and Soft Walls

When the bucket drops below thresholds:

- **Warning zone** (~70–85% consumed) — Light velocity throttling begins
- **Critical zone** (< 30% remaining) — Aggressive throttling + soft walls

**Velocity Throttling**

```pseudocode
function apply_throttling(bucket_remaining, effective_burn):
    if bucket_remaining > WARNING_THRESHOLD:
        return NO_THROTTLE
    elif bucket_remaining > CRITICAL_THRESHOLD:
        delay = (1 - bucket_remaining) * MAX_DELAY_FACTOR
        return {"action": "velocity_throttle", "delay_ms": delay}
    else:
        return {"action": "soft_wall", 
                "measures": ["reduce_max_agents", "partial_synthesis", "cooldown"]}
```

**Soft Walls** (final safety layer)
- Reduce output quality or resolution for multimodal tasks
- Force partial synthesis (return best-effort results from already-verified agents)
- Temporarily cap maximum active agents
- Introduce short cool-down periods

---

## Why This Matters for Korg

This token-bucket mechanism is the primary “kernel-level” control that makes the Heavy multi-agent runtime viable at cluster scale. It is the concrete realization of fair, policy-driven resource allocation that any production-grade multi-agent system must have.

It directly informs how a reference harness should expose cost, fairness, and throttling signals to operators and higher-level orchestrators.

---

## Related

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground truth architecture containing the original formula and context.
- [[wiki/reference-harness/Serving-Infrastructure-and-KV-Cache-Lifecycle.md]] — Complementary note on the memory and serving techniques that drive the burn rate.
- [[wiki/mechanisms/state-primitives.md]] and [[wiki/mechanisms/isolation-routing.md]] — The governance and isolation primitives that must coexist with cluster-level resource controls.