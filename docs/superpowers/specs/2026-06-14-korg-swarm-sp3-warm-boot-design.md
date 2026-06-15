# Korg Swarm — SP3: Real warm boot (Track B)

**Status:** Design+plan / approved-by-delegation ("do it all now"), grounded against real code 2026-06-14
**Branch:** `feat/swarm-warm-boot` (stacked on `feat/swarm-collaboration`)
**Sub-project:** SP3 of Track B (final).

## 1. The honest problem (grounded)
- `speculative_warm_boot` (`execution/dag.rs:213`) is a pure bool-setter — primes nothing.
- It sits on `SpeculativeScheduler::run`, which is **test-only** (no production caller). The real campaign work is `leader → dispatch_level → spawn_worker_process → korg worker child → observation::cargo_check` — a separate process whose `cargo check` runs cold in each worktree (`observation.rs:25`, no shared target dir).
- **The theater trap:** priming the scheduler primes a path production never runs. A warm boot is only *real* if the actual worker `cargo_check` demonstrably reuses what it warmed.
- The orphaned `SandboxPool` (pool.rs) primes a node/LSP world the cargo campaign never queries — wrong shape; leave it.

## 2. Goal
Make warm boot do **real** work that the **real worker path** reuses: pre-warm a shared `CARGO_TARGET_DIR` once, and point every worker's `cargo check` at it so compiled dependencies are reused across workers/rounds instead of recompiled cold. Gated behind `--speculative`, hermetic (no hang/fail on a bare host).

**Acid test:** with `--speculative`, the shared target dir is populated by warm boot and each worker process is spawned with `CARGO_TARGET_DIR` pointing at it (so `cargo check` reuses it). With it off (default), no warm boot runs and workers use their own target dir (unchanged behavior).

## 3. Design (both sides derive the SAME stable path — no fragile plumbing)
- New `pub fn warm_target_dir(session_id) -> PathBuf` (a stable per-campaign shared cargo target dir, e.g. under the OS cache dir / `~/.korg/cache/target-<session>`). Both the warm boot and the worker spawn compute this independently, so they agree without passing data across the process boundary.
- **Warm boot becomes real** (`dag.rs:speculative_warm_boot`, or a new `warm_boot` entry the campaign calls): create `warm_target_dir`, then warm it — run `cargo check` (or `cargo fetch` + `cargo metadata`) once against the campaign repo / fixture with `CARGO_TARGET_DIR=warm_target_dir`, so the dependency graph compiles into the shared cache. Store the path. Real work, demonstrably populating the cache.
- **Worker reuses it:** in `SubprocessBackend::spawn` (session.rs), when speculative is enabled, set `.env("CARGO_TARGET_DIR", warm_target_dir(session))` on the spawned `korg worker` process. `observation::cargo_check` needs NO change — `cargo` honors `CARGO_TARGET_DIR` from the env automatically. (If env-on-spawn is awkward, the worker sets it before `cargo_check`; prefer env-on-spawn.)

## 4. Slices
### Slice 1 — `warm_target_dir` + real warm boot (TDD)
- `warm_target_dir(session_id) -> PathBuf` (stable, unique per session, under a cache root). Test: same session → same path; different sessions → different.
- `pub async fn warm_boot(session_id, repo: &Path, enabled: bool) -> WarmBootReport` (in a new `execution/warm_boot.rs` or in dag.rs): if `!enabled` → no-op (returns `skipped`). If enabled: create `warm_target_dir`, run `cargo check` there with `CARGO_TARGET_DIR` set, **wrapped in `tokio::time::timeout`** and a cargo-presence check; on absence/timeout → log + return `unavailable` (NEVER hang/fail). Returns whether it populated the cache.
- Test: with cargo present + a tiny crate, warm_boot(enabled=true) creates a NON-EMPTY `warm_target_dir` (structural reuse proof — the cache is real); with enabled=false → dir not created, returns skipped; with a forced-absent cargo → returns unavailable, no panic.
- Replace the no-op `speculative_warm_boot` body to call the real `warm_boot` (or deprecate it in favor of the new entry called from the campaign).

### Slice 2 — Worker reuses the shared cache (the anti-theater link)
- In `SubprocessBackend::spawn` (session.rs), thread a `speculative: bool` (or read it) and, when on, set `.env("CARGO_TARGET_DIR", warm_target_dir(session))` on the worker `Command`.
- Test: a unit test asserting that when speculative is enabled, the spawn command's env contains `CARGO_TARGET_DIR == warm_target_dir(session)` (so the worker's cargo_check provably reuses the warmed cache); when off, it doesn't.

### Slice 3 — Gate `--speculative` end to end
- Thread the `speculative_execution` capability (resolver `active_states`) or a bool into the campaign so warm_boot + the worker env are conditional.
- Add a clap `--speculative` flag to `Cli` in `src/main.rs` (mirror `--inject-stress`) that enables it (capability default stays whatever it is today; document the on/off). Wire it to the leader so warm_boot + spawn-env are gated.
- Test/smoke: default (no flag) → no warm boot, no CARGO_TARGET_DIR env; `--speculative` → warm boot runs + env set.

## 5. Autonomous decisions
- **Shared path derivation over data-plumbing:** both warm boot and worker-spawn compute `warm_target_dir(session)` — avoids touching `SessionSpec`/ACP schema.
- **Reuse proven structurally, not by timing:** assert the cache dir is populated + the worker env points at it (no flaky speedup benchmark).
- **Hermetic contract:** cargo absent OR warm-boot timeout (cap ~60s) → degrade to cold path with a log, never hang/fail. The bare-host path must complete.
- **Do NOT wire SandboxPool** (wrong shape). Leave it as-is (or note as dead code for a later cleanup).
- **Default off** unless `--speculative` (so the default campaign path is unchanged; warm boot is opt-in).

## 6. Verification
- `cargo test -p korg-runtime` green (warm_target_dir, warm_boot real/skipped/unavailable, spawn-env gating).
- Default path unchanged (no warm boot, no env); `--speculative` populates the shared cache + sets the worker env.
- Hermetic: with cargo forced absent, warm_boot returns unavailable without hang/panic.
- Full `cargo test --workspace` green; fmt + clippy clean on touched code.
- Honesty: the warm boot does real compilation into a shared cache the worker provably reuses — not a no-op, not a prime on a dead path.

## 7. Out of scope
- Wiring/deleting `SandboxPool` (separate cleanup).
- Timing/speedup benchmarks (flaky); we prove reuse structurally.
- A persistent cross-campaign cache (per-session is enough).
