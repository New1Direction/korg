# korg-tui

Ratatui operator dashboard for live Korg campaigns.

`korg-tui` is the interactive terminal UI that renders a running
`LeaderOrchestrator` campaign: a tabbed dashboard with a workspace file
browser/editor, a swarm console, campaign observability panels, a git
timeline, and the interrupt-driven approval / rewind prompts. It is the
front-end for `korg tui`, `korg campaign --tui`, and `korg leader --demo --tui`
(the binary lives in the root `korg` crate's `src/main.rs`).

## What it does

The crate owns one big piece of mutable UI state, `KorgTui`, and an async
event loop, `run_tui_event_loop`, that:

1. Sets up the terminal (raw mode, alternate screen, mouse capture) with a
   panic hook that restores the terminal on crash.
2. On each tick, drains three `tokio::sync::mpsc` channels — orchestrator
   updates (`TuiUpdate`), background subprocess stdout, and swarm-agent
   replies — into `KorgTui` fields.
3. Renders the dashboard via `draw_dashboard`.
4. Reads crossterm key events and dispatches them to the focused panel
   (file tree, editor, console, command palette, modal, or rewind picker).

The UI talks to the orchestration layer over two channels defined in
**korg-runtime** (`korg_runtime::tui_bridge`), re-exported here for callers:

- `TuiUpdate` — pushed *from* the `LeaderOrchestrator` *to* the TUI
  (verdicts, arena rounds, trace/`.ktrans` events, contract negotiation,
  persona + scale telemetry, approval requests, and `RewindAvailable`).
  Every variant is mapped onto `KorgTui` state in the event loop's
  `match update { … }` block.
- `ContractResponse` — sent *from* the TUI *back* to the orchestrator
  (`Approve` / `Reject` / `Force` / `Override(Vec<String>)` / `Rewind(u64)`),
  e.g. when the operator answers an approval modal, runs a `/`-command, or
  picks a recovery point.

## Key types and functions

| Item | Role |
|:---|:---|
| `KorgTui` | The entire dashboard state struct: tab/focus, file tree, open editor tabs, console & terminal logs, git commits, command palette, telemetry fields, and the rewind picker. `KorgTui::default()` eagerly scans the CWD and `git log`. |
| `run_tui_with_campaign(prompt, session)` | Spawns a fresh `LeaderOrchestrator` campaign in the background and drives the TUI against it. Used by `korg tui` / `korg campaign --tui`. |
| `run_tui_with_leader(leader)` | Attaches the TUI to an already-constructed `LeaderOrchestrator`. Used by `korg leader --demo --tui`. |
| `highlight_line(line, ext)` | Syntect-based syntax highlighter returning a ratatui `Line<'static>`; backs the editor pane. Maps common extensions (`rs`, `py`, `ts`, `json`, `toml`, …) to a `base16-ocean.dark` theme. |
| `TuiTab` / `TuiFocus` | Enums for the four tabs (Workspace IDE, Swarm Console, Observability, Git Timeline) and which panel currently has focus. |
| `FileEntry` / `EditorTab` / `GitCommit` | Plain data backing the file browser, the multi-tab editor (modal vim-style normal/insert editing, `Ctrl+S` to save), and the git timeline. |
| `CommandCode` / `PaletteItem` / `CommandOption` | The `Ctrl+P` command palette: fuzzy-matched (`KorgTui::fuzzy_match`) commands, workspace files, and grep matches. |

`run_tui_event_loop` and `draw_dashboard` are private; callers go through the
two `run_tui_with_*` entry points.

### Editor / git / build integration

These parts touch the real environment, not mock data:

- The file tree and editor read and write the real working directory
  (`std::fs`); `target/`, `node_modules/`, `.git`, and most dotfiles are
  skipped.
- The git timeline shells out to `git log --oneline`.
- The command palette and `Ctrl+B` / `Ctrl+T` spawn real `cargo build` /
  `cargo test` (and `git status` / `git diff`) subprocesses via
  `tokio::process`, streaming their output into the terminal pane.

## Where it fits in the workspace

```
korg (root bin, src/main.rs)
  └─ korg_tui::run_tui_with_campaign / run_tui_with_leader
        ├─ korg-runtime  ─ LeaderOrchestrator (the campaign engine)
        │                  tui_bridge::{TuiUpdate, ContractResponse}
        │                  recovery::{RewindCandidate, RewindScope}
        │                  acp::AcpMessage
        └─ korg-core
```

`korg-tui` depends only on **korg-core** and **korg-runtime**. It has no
knowledge of the ledger internals — it consumes orchestrator events and emits
operator responses. The web equivalent is `korg-server` (Axum + SSE), which
its source notes is the cockpit counterpart to `run_tui_with_campaign`.

## Usage

There is no standalone binary; the crate is driven from the root `korg` CLI.
To embed it directly:

```rust
use korg_runtime::leader::LeaderOrchestrator;

// Spawn a campaign and render it:
korg_tui::run_tui_with_campaign(
    "Refactor the auth layer to use JWTs".to_string(),
    None, // session: Option<uuid::Uuid>
).await?;

// Or attach to a leader you built yourself:
let leader = LeaderOrchestrator::new(prompt, session);
korg_tui::run_tui_with_leader(leader).await?;
```

Key bindings (selected): `Ctrl+P` command palette, `Ctrl+B`/`Ctrl+T`
build/test, `Ctrl+L` clear console, `?` help modal, `1`–`4` switch tabs,
`Ctrl+R` on-demand rewind picker, vim-style `h/j/k/l` + `i`/`Esc` in the
editor.

## Status / caveats

This crate is feature-rich on the rendering side but the live data path is
partial — read these honestly before relying on a panel:

- **All `TuiUpdate` variants are wired** to `KorgTui` state, so any panel the
  orchestrator actually emits for will update live.
- **The campaign spawn is acknowledged as a workaround.** `run_tui_with_campaign`
  notes in-code that proper event hooks on `LeaderOrchestrator` don't exist
  yet, so it "runs the campaign and periodically sends updates."
- **Telemetry, persona scores, lock states, and sparkline histories default to
  empty/zero** in `KorgTui::default()` and are populated only from real
  `TuiUpdate` signals — never seeded or fabricated. Before the first update,
  panels read as empty rather than showing demo data (`current_verdict`
  honestly defaults to "Waiting for first evaluation...").
- **The git timeline reads real `git log` metadata;** on any failure (no git,
  non-zero exit, or empty output) it leaves the timeline empty rather than
  fabricating commits.
- `fuzzy_match` exists both as a `KorgTui` method (used by the palette) and as
  a duplicate free function at the bottom of the file.

`Cargo.toml` declares a `cli` feature (default-on) that is currently inert —
no code is gated on it.
