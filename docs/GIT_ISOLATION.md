# High-Assurance Git Sandbox Isolation & Low-Level Index Specification

Korg implements a **Zero-Trust Physical Sandbox Isolation Architecture** utilizing low-level Git plumbing commands and ephemeral worktree isolation. This specification details the mathematical and structural guarantees Korg provides to protect host repositories from state contamination, prevent lock contention during parallel swarm execution, and ensure deterministic, content-addressed workspace rollbacks.

---

## 🏛️ 1. Why Passive Directory Chaining Fails

Traditional autonomous agents (such as Aider, AutoGen, or CrewAI) execute code modifications directly inside the active host working directory. This passive model introduces critical vulnerabilities:

1. **State Leakage & Branch Pollution**: Stale, uncommitted mutations, or partial compilation artifacts can contaminate subsequent execution tasks or pollute the active branch state.
2. **Execution Race Conditions**: Running multiple agents or parallel persona swarms in the same directory leads to write-lock conflicts, filesystem contentions, and overlapping modifications.
3. **Imperfect Rollbacks**: If a test suite fails or a critic rejects a patch, reverting changes requires parsing diffs or executing heavy `git reset --hard` commands, which can wipe out valid manual developer work.

Korg resolves this by separating logical execution from the physical repository state, treating the host codebase as a read-only base image and delegating all mutations to transient physical sandboxes.

---

## 🔄 2. Transient Worktree Lifecycle Specification

Korg isolates every worker process's runtime environment inside an ephemeral sandbox directory located under `/tmp/korg/worktrees/<worker_id>-<routing_id>`. The lifecycle of this physical containment follows four distinct phases:

```text
  [Host Base Repository]
            │
            ▼ (git rev-parse base_snapshot)
  ┌──────────────────────────────────────────────────────────┐
  │ Phase 1: Reference Validation                           │
  │ Verify base snapshot exists, fallback to HEAD if invalid │
  └─────────────────────────┬────────────────────────────────┘
                            │
                            ▼ (git worktree add -f -B <branch> <path> <snapshot>)
  ┌──────────────────────────────────────────────────────────┐
  │ Phase 2: Sandbox Provisioning                            │
  │ Create isolated branch & mount physical git worktree    │
  └─────────────────────────┬────────────────────────────────┘
                            │
                            ▼ (git write-tree)
  ┌──────────────────────────────────────────────────────────┐
  │ Phase 3: Zero-Trust Containment Check                    │
  │ Assert workspace layout equals expected codebase hash    │
  └─────────────────────────┬────────────────────────────────┘
                            │
                            ▼ (Execute Swarm Campaign)
  ┌──────────────────────────────────────────────────────────┐
  │ Phase 4: Ephemeral Teardown                              │
  │ Restore CWD, git worktree remove --force, git branch -D  │
  └──────────────────────────────────────────────────────────┘
```

### Phase 1: Reference Validation
Before spawning a worker sandbox, Korg asserts the integrity of the requested parent target snapshot reference (defaulting to `HEAD`). It executes a low-level verification scan:

```bash
git rev-parse --verify <base_snapshot>
```

If the reference exists, the hash is captured as the immutable base image snapshot; otherwise, the runtime falls back gracefully to `HEAD`.

### Phase 2: Sandbox Provisioning
Korg dynamically creates an isolated development branch and mounts a physical, concurrent Git worktree to a transient directory:

```bash
git worktree add -f -B korg-branch-<routing_id> /tmp/korg/worktrees/<worker_id>-<routing_id> <snapshot_ref>
```

This allocates a distinct, concurrent working copy linked to the same `.git` database, enabling isolated multi-agent writing without directory duplication.

### Phase 3: Zero-Trust Containment Check
Once mounted, the worker process's current working directory (CWD) is restricted to the sandbox root. To ensure that no external filesystem drift has occurred, Korg runs a low-level index check:

```bash
git write-tree
```

The resulting SHA-1/SHA-256 tree hash is compared against the expected `codebase_merkle_root` inside the incoming task metadata. If the hashes mismatch, the runtime aborts instantly, preventing execution on corrupted code layouts:

$$\text{Actual Workspace Tree Hash} \equiv \text{Expected Codebase Merkle Root}$$

### Phase 4: Ephemeral Teardown
Upon successful execution and validation of modifications in the adversarial arena, the worker process restores the parent repository working directory, evicts the physical worktree folder, and deletes the temporary branch:

```bash
# Evict physical worktree files
git worktree remove --force /tmp/korg/worktrees/<worker_id>-<routing_id>

# Prune branch reference
git branch -D korg-branch-<routing_id>
```

---

## 🎛️ 3. Low-Level Index & Merkle-DAG State Hashing

Korg leverages Git's underlying content-addressed object database to guarantee absolute determinism and atomic, transaction-style commits.

### The Tree Object Hashing Protocol
When a worker successfully completes a plan execution stage, Korg stages the modifications and computes the physical tree hash structure:

```bash
git add .
git write-tree
```

Git's `write-tree` command operates directly on the directory index, packing modifications into content-addressed blobs and structuring them into hierarchical tree files. The resulting hash represents a mathematically reproducible Merkle tree representing the entire codebase layout.

### Historical State Restoration (Replay Scrubber)
During timeline time-travel replay or manual operator branching, Korg reconstructs the historical layout of the workspace by resetting the development index directly to the historical codebase Merkle root:

```bash
git read-tree --reset -u <codebase_merkle_root>
```

This bypasses traditional file-by-file patching, using Git's low-level tree matching algorithm to reset the directory layout to the exact state of the historical transaction in a single low-level tree operation, while preserving the logical CRDT Blackboard state by importing the matching state-blob.

---

## 🚨 4. Crash Preservation & Forensic Diagnostic Logs

Autonomous campaigns are subject to third-party LLM failures, compile panics, or runtime anomalies. Korg balances reliability with developer visibility through its **Fail-Safe Crash Forensic Preservation Protocol**.

If a worker process encounters a non-zero exit code or trigger panic:

1. **Campaign Lock**: The Orchestration Kernel immediately locks the transaction log, suspending the campaign timeline to prevent state corruption.
2. **Teardown Bypass**: The standard worktree removal and branch deletion sequences are bypassed.
3. **Forensic Preservation**: The transient sandbox folder at `/tmp/korg/worktrees/<worker_id>-<routing_id>` is left fully intact, keeping all intermediate compile artifacts, failed test logs, and raw diffs in place.
4. **Attestation Log**: A detailed diagnostic report is printed to the operator, including the exact local absolute path of the preserved workspace for immediate manual debugging:

```text
[Harness] CRASH DETECTED: Worker Lucas exited with status 101.
[Harness] Fail-Safe Activation: Ephemeral worktree teardown bypassed.
[Harness] Preserving sandbox state for developer forensics...
[Harness] Preserved Workspace Path: /tmp/korg/worktrees/lucas-test-route-123
[Leader] Spawning recovery node. Continuing logical execution...
```

5. **Logical Recovery**: The Orchestration Kernel reads the last canonical, signed transaction record (`.ktrans.json`) from the Blackboard, provisions a fresh sibling worktree, and seamlessly continues the campaign from the last validated checkpoint.
