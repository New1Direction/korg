---
date: 2026-05-20
type: reference-implementation
tags: [reference-implementation, pseudocode, acp, harness, leader-broker]
harness: korg
domain: acp, reference-implementation, tooling
status: active
ai-first: true
---

# Minimal ACP Client Pseudocode

This note provides a minimal, working pseudocode reference for a harness that speaks the ACP defined in the Grok 4.20 Heavy architecture (see [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]]).

It demonstrates the core responsibilities any Korg-compliant harness must implement: worktree isolation, tool proxying, blackboard interaction, `.ktrans` emission, and participation in Leader-Broker coordination.

---

## Purpose

The goal is to give implementers a concrete starting point that is:

- Faithful to the real Grok Heavy ACP and Leader-Broker model
- Aligned with the three Korg mechanism contracts (`state-primitives.md`, `isolation-routing.md`, `transactional-memory.md`)
- Simple enough to understand in one sitting
- Extensible toward full Grok Build-style plan/review/approve + Arena loops with native 4-persona specialization (Captain, Harper, Benjamin, Lucas)

---

## 1. Minimal ACP Client (Core Loop)

```pseudocode
class MinimalACPClient:
    def __init__(self, role: "leader" | "worker" | "broker", worker_id: str = None):
        self.role = role
        self.worker_id = worker_id or generate_uuid()
        self.blackboard = {}  # local view (synced via ACP)
        self.worktrees = {}   # worker_id -> path

    def connect(self, endpoint: str):
        # Open ACP session (stdio, Unix socket, WebSocket, etc.)
        self.transport = ACPTransport(endpoint)
        self.transport.send({
            "type": "WorkerHello" if self.role == "worker" else "BrokerHello",
            "worker_id": self.worker_id,
            "claimed_capabilities": ["code", "search", "exec"]
        })

    def run(self):
        while True:
            msg = self.transport.receive()
            if msg["type"] == "RouteWork":
                self.handle_route_work(msg)
            elif msg["type"] == "RequestTerminate":
                self.handle_terminate(msg)
            elif msg["type"] == "BlackboardPatch":
                self.apply_blackboard_patch(msg)
            elif msg["type"] == "ConflictResolve":
                self.participate_in_arena(msg)
```

---

## 2. Single-Worker Harness Example

```pseudocode
class SingleWorkerHarness(MinimalACPClient):
    def __init__(self):
        super().__init__(role="worker")
        self.current_worktree = None

    def handle_route_work(self, msg):
        # 1. Create isolated worktree
        task_id = msg["task_id"]
        self.current_worktree = create_git_worktree(f"/tmp/korg/worktrees/{self.worker_id}")
        mount_verified_snapshot(self.current_worktree, msg["base_snapshot"])

        # 2. Run the actual work (user code / persona logic)
        result = self.run_task_in_worktree(msg["payload"])

        # 3. Emit .ktrans (micro or terminal)
        tx = build_ktrans(
            worker_id=self.worker_id,
            task_id=task_id,
            base_snapshot=msg["base_snapshot"],
            mutations=result["mutations"],
            provenance=...,
            doom_loop_detected=result.get("doom_loop", False)
        )
        self.transport.send({
            "type": "SubmitTransaction",
            "tx_id": generate_uuidv7(),
            "content_hash": hash_canonical(tx),
            "payload": tx
        })

        # 4. Report termination
        self.transport.send({
            "type": "TerminationReport",
            "worker_id": self.worker_id,
            "exit_status": "success",
            "terminal_tx_id": tx["tx_id"]
        })

    def run_task_in_worktree(self, payload):
        # This is where the actual agent logic lives (researcher, implementer, etc.)
        # It can read/write only inside self.current_worktree
        ...
        return {"mutations": [...], "doom_loop": False}
```

---

## 3. Hello-World Multi-Agent Example (Leader + 2 Workers)

```pseudocode
# Leader side (minimal)
leader = MinimalACPClient(role="leader")
leader.connect("internal-acp-endpoint")

# Spawn two workers via ACP
leader.transport.send({
    "type": "RouteWork",
    "routing_id": "task-001",
    "capabilities": ["research"],
    "base_snapshot": "abc123...",
    "epoch_deadline": "..."
})

leader.transport.send({
    "type": "RouteWork",
    "routing_id": "task-002",
    "capabilities": ["implement"],
    "base_snapshot": "abc123...",
    "epoch_deadline": "..."
})

# Receive results and run Arena if needed
while True:
    msg = leader.transport.receive()
    if msg["type"] == "SubmitTransaction":
        # Merge into blackboard (see merge_blackboard_write pseudocode in ground-truth)
        ...
    if msg["type"] == "ConflictNotification":
        leader.run_arena_mode(msg)
```

---

## 4. Complete Grok Build-Style End-to-End Reference Workflow (Plan → Parallel Worktrees → Arena → Human Gates → Semantic Merge + Session Lifecycle & Recovery)

This is the **primary practical reference** in the harness layer. It shows a realistic, production-aligned implementation of the full interactive + recoverable Grok Build flow using the ACP, the three Korg mechanism contracts, and the exact patterns from the Grok 4.20 Heavy ground truth.

The flow is deliberately written so a serious implementer can translate it almost line-for-line into a real CLI/daemon.

---

### 4.1 SessionManager — Lifecycle, Checkpointing & Recovery

The session is the unit of resumption. All durable state lives in the blackboard + an ordered log of `tx_id`s.

```pseudocode
class SessionManager:
    def __init__(self, session_id: str, leader: "LeaderOrchestrator"):
        self.session_id = session_id
        self.leader = leader
        self.checkpoints = {}          # task_id -> last verified tx_id
        self.pending_work = {}         # routing_id -> RouteWork payload
        self.recovered = False

    def create_or_resume(self, user_prompt: str = None, resume_from: str = None):
        if resume_from:
            self.session_id = resume_from
            self.recovered = True
            self.replay_from_last_checkpoint()
            self.rehydrate_pending_work()
            self.leader.transport.send({"type": "SessionResumed", "session_id": self.session_id})
        else:
            self.session_id = generate_uuidv7()
            self.leader.blackboard.set(f"session/{self.session_id}/root_prompt", user_prompt)
            self.checkpoint("root", None)

    def checkpoint(self, scope: str, tx_id: str | None):
        self.checkpoints[scope] = tx_id or "verified_head"
        self.leader.blackboard.set(
            f"session/{self.session_id}/checkpoint/{scope}",
            {"tx_id": tx_id, "ts": now()}
        )

    def replay_from_last_checkpoint(self):
        last_tx = self.leader.blackboard.get(f"session/{self.session_id}/last_verified_tx")
        for tx in self.leader.blackboard.replay_transactions_since(last_tx):
            self.leader.apply_transaction(tx, is_replay=True)

    def rehydrate_pending_work(self):
        pending = self.leader.blackboard.get(f"session/{self.session_id}/pending_routes") or []
        for route in pending:
            self.leader.dispatch_work(route, is_recovery=True)

    def record_terminal_ktrans(self, worker_id: str, tx: dict):
        # Always append; the Broker will order by tx_id (UUIDv7)
        self.leader.blackboard.append(f"session/{self.session_id}/terminal_tx", tx)
        if tx.get("doom_loop_detected"):
            self.leader.handle_doom_loop(worker_id, tx)
```

---

### 4.2 LeaderOrchestrator — Full Plan / Dispatch / Arena / Approve / Merge Loop

```pseudocode
class LeaderOrchestrator(MinimalACPClient):
    def __init__(self):
        super().__init__(role="leader")
        self.session_mgr = SessionManager(None, self)
        self.active_worktrees = {}     # routing_id -> {path, worker_id, base_snapshot}
        self.arena_results = {}

    def run_full_campaign(self, user_prompt: str, resume_session: str = None):
        self.connect("acp://localhost:7331")
        self.session_mgr.create_or_resume(user_prompt, resume_session)

        # === PHASE 1: Plan Presentation + User Approval ===
        plan = self.generate_structured_plan(user_prompt)          # returns DAG + work packages + authority hints
        root_task_id = generate_uuidv7()

        self.transport.send({
            "type": "PlanPresentation",
            "session_id": self.session_mgr.session_id,
            "task_id": root_task_id,
            "plan": plan,
            "estimated_burn": self.estimate_effective_burn(plan),  # token-bucket awareness
            "requires_approval": True,
            "approval_deadline": now() + 5*60
        })

        approval = self.wait_for_human_decision(root_task_id)     # blocks until task.approve / task.reject

        if approval["decision"] == "reject":
            self.emit_final_ktrans("campaign_rejected", {"reason": approval.get("rationale")})
            return "Campaign cancelled"

        if approval.get("edited_plan"):
            plan = self.reconcile_edited_plan(plan, approval["edited_plan"])

        self.session_mgr.checkpoint("plan_approved", None)

        # === PHASE 2: Parallel Dispatch into Isolated Worktrees ===
        for pkg in plan.work_packages:
            self.dispatch_work({
                "routing_id": pkg.id,
                "capabilities": pkg.personas,   // Common Grok Build set: ["reasoning/captain", "critique/harper", "tool-use/benjamin", "synthesis/lucas"] or subsets thereof
                "payload": pkg.description,
                "base_snapshot": self.get_verified_head(),
                "permissions": pkg.permissions,           # fs:worktree-only, net:read-only, etc.
                "authority_vector": pkg.authority,        # used by Merge-Arbitration
                "epoch_deadline": pkg.deadline
            })

        # === PHASE 3: Collection + Arena on Contested Results ===
        results = self.collect_all_results_with_timeout(plan.max_wall_time)

        contested = [r for r in results if r.state == "CONTESTED" or r.has_internal_conflict()]
        if contested:
            arena_outcome = self.run_arena_mode(contested)        # detailed below
            self.arena_results = arena_outcome
            synthesis = self.produce_hybrid_or_winner(arena_outcome)
        else:
            synthesis = self.fast_path_merge(results)

        # === PHASE 4: Human Review / Approve Gate (task.approve / task.reject) ===
        self.transport.send({
            "type": "ApprovalRequest",
            "session_id": self.session_mgr.session_id,
            "task_id": root_task_id,
            "ranked_candidates": synthesis.candidates,     # each carries score vector + rationale
            "arena_scores": self.arena_results,
            "diff_previews": [self.render_diff(c) for c in synthesis.candidates],
            "requires_approval": True
        })

        final = self.wait_for_human_decision(root_task_id)   # rich payload

        if final["decision"] == "reject":
            self.handle_reject(final)
            return

        selected = final.get("selected", synthesis.default_winner)

        # === PHASE 5: Semantic Merge to Main Tree + Final .ktrans ===
        merge_result = self.perform_semantic_merge(
            selected,
            base_snapshot=self.get_verified_head(),
            authority_override=final.get("authority_override")
        )

        final_tx = self.build_terminal_ktrans(
            worker_id="leader",
            mutations=merge_result.committed_mutations,
            provenance=merge_result.provenance,
            arena_scores=self.arena_results if contested else None
        )
        self.transport.send({"type": "SubmitTransaction", "payload": final_tx})
        self.session_mgr.record_terminal_ktrans("leader", final_tx)

        self.transport.send({"type": "CampaignComplete", "session_id": self.session_mgr.session_id})
```

---

### 4.3 Detailed Parallel Worker (Worktree + Micro + Terminal .ktrans)

```pseudocode
class FullWorktreeWorker(MinimalACPClient):
    def __init__(self, worker_id):
        super().__init__(role="worker", worker_id=worker_id)
        self.worktree = None
        self.micro_tx_log = []

    def handle_route_work(self, route):
        self.worktree = create_git_worktree(f"/tmp/korg/worktrees/{self.worker_id}-{route['routing_id']}")
        mount_verified_snapshot(self.worktree, route["base_snapshot"])

        try:
            result = self.execute_persona_logic(route["payload"], route["permissions"])

            # Micro-transactions throughout execution (see transactional-memory.md)
            for mutation in result.incremental_mutations:
                tx = self.build_micro_ktrans(route, mutation)
                self.micro_tx_log.append(tx)
                self.transport.send({"type": "SubmitTransaction", "payload": tx})   # Broker merges immediately

            # Final terminal transaction (mandatory on every exit path)
            terminal = self.build_terminal_ktrans(route, result.final_state, doom_loop=result.doom_loop)
            self.transport.send({"type": "SubmitTransaction", "payload": terminal})
            self.transport.send({
                "type": "TerminationReport",
                "worker_id": self.worker_id,
                "routing_id": route["routing_id"],
                "exit_status": "success" if not result.doom_loop else "doom_loop",
                "terminal_tx_id": terminal["tx_id"]
            })

        except Exception as e:
            # Still emit diagnostic terminal .ktrans so nothing is lost
            diag = self.build_terminal_ktrans(route, {"error": str(e)}, is_crash=True)
            self.transport.send({"type": "SubmitTransaction", "payload": diag})
            self.transport.send({"type": "TerminationReport", "exit_status": "crash", ...})

        finally:
            self.cleanup_worktree()   # git worktree remove (or leave for forensics)
```

Key contract: **every coherent inference or observation** triggers a micro `.ktrans`; every termination (graceful, panic, SIGKILL shim, or Broker kill) triggers exactly one terminal `.ktrans`.

---

### 4.4 Arena Mode Participation & Self-Scoring (Merge-Arbitration Engine)

```pseudocode
def run_arena_mode(self, contested_items):
    # Each worker already attached provenance + self-score vector to its results
    scored = []
    for item in contested_items:
        vector = self.compute_self_score_vector(item)   # {correctness, completeness, novelty, minimal_diff, provenance_strength}
        scored.append({"item": item, "self_score": vector})

    # Leader aggregates (weighted by authority_vector + historical reliability)
    aggregated = self.weighted_aggregate(scored)        # see state-primitives.md authority matrix

    if aggregated.has_clear_winner(threshold=0.75):
        return {"mode": "winner", "winner": aggregated.best, "scores": aggregated}

    # Otherwise produce hybrid synthesis (Leader meta-reasoning + best fragments)
    hybrid = self.synthesize_hybrid(aggregated)
    return {"mode": "hybrid", "hybrid": hybrid, "scores": aggregated, "tie_break_rationale": "..."}

def compute_self_score_vector(self, item):
    # In real Grok Build this is produced by the model itself under the worker persona
    return {
        "correctness": item.verified_against_blackboard(),
        "completeness": item.coverage_of_requirements(),
        "novelty": 1.0 - item.similarity_to_existing(),
        "minimal_diff": item.diff_size_penalty(),
        "provenance_strength": item.provenance_depth()
    }
```

This matches the "Arena Mode = Merge-Arbitration Engine" clarification and the authority-vector resolution in `state-primitives.md`.

---

### 4.5 Human Review Gates — `task.approve` and `task.reject`

The thin CLI client (or any front-end) translates user actions into these two ACP messages.

**Plan level:**

```json
{
  "type": "task.approve",
  "task_id": "...",
  "session_id": "...",
  "edited_plan": { ... } | null,
  "rationale": "I want to drop the security-audit package for now"
}
```

**Final synthesis level:**

```json
{
  "type": "task.approve",
  "task_id": "...",
  "session_id": "...",
  "selected": "hybrid-3" | "candidate-2" | "diff:sha256:...",
  "authority_override": { "reason": "user_trust", "weight": 1.2 },
  "rationale": "The hybrid combines the best of A and C"
}
```

**Reject:**

```json
{
  "type": "task.reject",
  "task_id": "...",
  "session_id": "...",
  "reason_code": "plan_too_broad" | "security_concern" | "user_cancel",
  "rationale": "free-text or structured"
}
```

The Leader treats both messages as first-class control signals. `task.reject` can still trigger a partial `.ktrans` so the session record is not empty.

---

### 4.6 Semantic Merge Back to Main Tree

```pseudocode
def perform_semantic_merge(self, selected_candidate, base_snapshot, authority_override=None):
    mutations = selected_candidate.proposed_mutations
    for m in mutations:
        result = self.leader.blackboard.merge_blackboard_write(
            key=m.target_path,
            new_value=m.payload,
            source_agent="arena-synthesis" if self.arena_results else "worker-direct",
            authority_vector=authority_override or m.authority_vector,
            base_snapshot=base_snapshot
        )
        if result == "PENDING_ARBITRATION":
            # Will be handled by a subsequent human review or higher authority
            ...
    return {"committed_mutations": ..., "provenance": ...}
```

This delegates to the exact `merge_blackboard_write` + three-way rebasement logic defined in the ground-truth and `transactional-memory.md`.

---

### 4.7 Session Recovery on Disconnect / Crash / Reconnect

```pseudocode
# CLI / operator side
if user_runs_with_session_flag:
    leader.run_full_campaign(None, resume_session="019e43...")

# Inside Leader (on reconnect)
if self.session_mgr.recovered:
    # Re-attach any still-live worktrees that the OS preserved
    for route in self.session_mgr.pending_work.values():
        if worktree_still_exists(route):
            self.rebind_worker_to_worktree(route)
        else:
            # Re-dispatch with same routing_id but fresh snapshot + "resume_from_tx" hint
            self.dispatch_work(route, is_recovery=True, resume_hint=last_known_tx)
```

Recovery guarantees (per the contracts):

- No epistemic work is lost (micro + terminal `.ktrans` rule).
- Replayed transactions are idempotent (UUIDv7 + etag checks).
- Pending RouteWork can be safely re-issued.
- Human approval gates that were never answered are re-presented.

---

## 5. Mapping, Extension Points & Implementation Guidance

### Direct Mapping to Korg Contracts

- Session lifecycle & recovery → `transactional-memory.md` (`.ktrans` replay + rebasement)
- Worktree creation + cleanup → `isolation-routing.md`
- Epistemic promotion + Arena → `state-primitives.md` (Merge-Arbitration Engine)
- Human gates (`task.approve`/`task.reject`) → ACP-Binding-Design + Grok-Build-CLI-Internals
- Resource awareness (burn estimate) → Token-Bucket-Throttling-and-Resource-Gating.md

### Extension Points (prioritized for real harnesses)

1. `result.stream` + incremental provenance for live TUI dashboards
2. Full headless daemon mode (`--session <id> --auto-approve-threshold 0.82`)
3. Dynamic scaling (`should_scale` signal when entropy or conflict rate spikes)
4. `capability.revoke` mid-campaign + safe rollback to last checkpoint
5. KV-cache / token-bucket observability hooks for cost dashboards
6. Multi-node / distributed Broker (the current sketch assumes co-located Leader-Broker)

### How to Use This Note

- Start with sections 1–3 for the absolute minimal ACP client.
- Use **section 4** as the blueprint for any serious Grok Build-style or Korg-compliant orchestrator.
- Every major class and message here is deliberately traceable to the ground-truth architecture and the three mechanism contracts.

This level of completeness turns the reference-harness layer from "excellent documentation" into "something people can actually build a production harness against."

## Related

- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — External validation of the Planner + Generator + Evaluator structure and adversarial evaluation loops that the 4-persona specialization and LeaderOrchestrator in this note implement.

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Primary ground truth (all pseudocode here implements its ACP + Arena + recovery model).
- [[wiki/reference-harness/ACP-Binding-Design.md]], [[wiki/reference-harness/ACP-Message-Schema.md]], [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]]
- [[wiki/reference-harness/ACP-v1.17-Wire-Format.md]] — The precise JCS + Ed25519 wire format this pseudocode targets for a production-grade implementation.
- [[wiki/reference-harness/Grok-Build-CLI-Internals.md]] — The real Rust-side realization of the thin client + plan/review/approve UX.
- [[wiki/reference-harness/Token-Bucket-Throttling-and-Resource-Gating.md]] and [[wiki/reference-harness/Serving-Infrastructure-and-KV-Cache-Lifecycle.md]] — Resource model referenced in the burn estimation and throttling paths.
- [[wiki/mechanisms/state-primitives.md]], [[wiki/mechanisms/isolation-routing.md]], [[wiki/mechanisms/transactional-memory.md]] — The three contracts this harness is built on.
- [[Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md]] — The operator guide that describes exactly what a human should watch while a harness built from this pseudocode is running a real Heavy campaign.

The note is intentionally self-contained enough that a new harness author can start coding after reading only this file + the ground-truth + the triad.

A real, compilable Rust reference implementation that follows this pseudocode is available at:

**`reference-implementations/rust/grok-acp-harness/`**

Run it with:
```
cd reference-implementations/rust/grok-acp-harness
cargo run -- worker --id my-worker-01
```

---

## 6. Grok Build-Style 4-Persona Specialization (Captain, Harper, Benjamin, Lucas)

This section shows how the generic harness maps onto the native 4-agent topology used in real Grok 4.20 Heavy and exposed by Grok Build (see the operator guide in [[Human/Methodology/How-to-Watch-a-Live-16-Agent-Campaign.md]]).

### Example Work Package Decomposition

```pseudocode
plan = {
    "root_task": "Refactor authentication module for better auditability",
    "work_packages": [
        {
            "id": "pkg-captain-001",
            "personas": ["captain", "reasoning"],
            "description": "Decompose the task into a minimal viable DAG and produce the initial PlanPresentation",
            "authority": "high",
            "permissions": ["plan:write", "blackboard:read"]
        },
        {
            "id": "pkg-harper-001",
            "personas": ["harper", "critique"],
            "description": "Research prior art, find security and compliance gaps, produce counter-evidence",
            "authority": "medium",
            "permissions": ["web:search", "blackboard:read", "fs:read-only"]
        },
        {
            "id": "pkg-benjamin-001",
            "personas": ["benjamin", "tool-use"],
            "description": "Implement the refactored auth code inside an isolated worktree",
            "authority": "medium",
            "permissions": ["fs:write:worktree-only", "exec:local", "git:commit"]
        },
        {
            "id": "pkg-lucas-001",
            "personas": ["lucas", "synthesis"],
            "description": "Cross-validate outputs, run Arena on conflicts, prepare ranked candidates for ApprovalRequest",
            "authority": "high",
            "permissions": ["blackboard:read", "arena:participate"]
        }
    ]
}
```

### Dispatch with Persona Specialization

```pseudocode
for pkg in plan.work_packages:
    self.dispatch_work({
        "routing_id": pkg.id,
        "capabilities": pkg.personas,          # e.g. ["harper", "critique"]
        "payload": pkg.description,
        "base_snapshot": self.get_verified_head(),
        "authority_vector": pkg.authority,
        "permissions": pkg.permissions
    })
```

Workers declare their persona at `WorkerHello` time so the Leader (and human operator) can track which role is producing which signals — exactly the view described in the 16-agent operator guide (token velocity per persona, Arena participation per persona, terminal `.ktrans` richness per persona).

### Thin Client Surfaces for Operators

The CLI/TUI side of the harness should expose the same signals the operator guide recommends watching:

- Live PlanPresentation and ApprovalRequest events (with full Arena score vectors)
- Per-persona token velocity and micro-`.ktrans` rate
- Current CONTESTED count and rebasement pressure
- Worker lifecycle (especially productive deaths with rich terminal transactions)

This closes the loop between the reference implementation and the human operator experience.

---

## How This Pseudocode Fits the Overall Korg Vision

Sections 1–3 give the absolute minimal building blocks.
Section 4 gives the complete recoverable Grok Build-style campaign loop.
Section 6 shows how to specialize it for the actual 4-persona (and 16-agent) topology used in production.

Implementers can start with the minimal client, adopt the full LeaderOrchestrator when they need plan/review/approve safety, and then layer on persona specialization when targeting real Heavy swarms or Grok Build compatibility.

At this point the note is one of the most complete, traceable, and immediately usable reference artifacts in the entire vault.
