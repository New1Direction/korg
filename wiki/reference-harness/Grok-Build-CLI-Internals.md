---
date: 2026-05-20
type: reference-architecture
tags: [reference-architecture, grok-build, cli, worktree, headless, acp]
harness: korg
domain: acp, reference-implementation, tooling
status: active
ai-first: true
---

# Grok Build CLI Internals

**Grok Build** is the official terminal-based agentic coding tool that exposes the full Grok 4.20 Heavy multi-agent swarm (Leader + up to 16 parallel specialized runtimes) directly in the developer workflow. It is not a thin wrapper around chat; it is a first-class client of the ACP layer that drives the same backend orchestration used in chat Heavy mode, but with stronger emphasis on local filesystem isolation, plan/review/approve safety gates, and headless/scripted usage.

---

## Worktree Management

For any task that involves code modification, the Leader spawns each sub-agent in its own isolated git worktree (via `git worktree add`). This gives every agent a private, clean copy of the repository state. Sub-agents can edit, build, test, and commit in parallel without stepping on each other or the main tree.

The Leader maintains a central index of worktrees and merges approved changes back via clean diffs or interactive review. Worktrees are automatically cleaned up on task completion or failure.

---

## Tool Proxying

All tool calls (file read/write, shell exec, git operations, web search, code execution, etc.) are proxied through the Leader via ACP `tool.invoke` messages. The Leader enforces capability scopes, runs commands in sandboxed environments when possible, and streams stdout/stderr + structured results back to the requesting agent.

This prevents direct filesystem or network access from sub-agents and allows the Leader to audit every action.

---

## Plan / Review / Approve Loops

The default workflow is explicitly staged:

1. Leader decomposes the task and creates a high-level plan (DAG).
2. Plan is presented to the user (or auto-approved in headless mode with flags).
3. Sub-agents execute in parallel within their private worktrees.
4. Changes are collected as diffs → presented in Arena Mode for self-scoring/ranking.
5. Leader surfaces ranked diffs for human review/approve (or auto-applies safe changes based on policy).
6. Approved changes are merged back to the main tree; rejected ones are discarded with full provenance.

The loop is iterative and can be paused/resumed mid-task.

---

## How the CLI Drives the ACP Layer

The CLI binary (Rust-based) opens a persistent ACP session (stdio for interactive TUI, WebSocket/HTTP for remote/headless). It acts as a thin client:

- Translates user input → ACP `task.create` (root task)
- Streams `result.stream` events into the TUI or JSON output
- Forwards user approvals as `task.approve` or `task.reject`

All sub-agent lifecycle, blackboard updates, and conflict resolution happen on the backend; the CLI only renders and mediates human-in-the-loop points.

---

## Related

- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — The backend Leader + ACP architecture that the CLI drives.
- [[wiki/reference-harness/ACP-Binding-Design.md]] — The protocol surface used by the CLI.
- [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] — The coordination model exposed through the CLI.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Practical pseudocode harness that demonstrates the same patterns the CLI uses, including the full plan/review/approve + Arena loop.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — Workflow patterns commonly executed via Grok Build.