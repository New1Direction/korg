# korg demo tape — runs the REAL binary, zero simulation.
#
# Every command below is the actual `korg` / `korg-verify` binary doing real
# work: a real patch applied to a real git repo, a real `cargo check`, a real
# hash-chained korg-ledger@v1 journal independently verified, and a real rewind.
# There is no simulation script — nothing here is pre-scripted output.
#
# Regenerate:  cargo build --release && \
#              cargo build --release -p korg-verify && \
#              vhs demo.tape
# (assumes ./target/release/korg and ./target/release/korg-verify exist)

Output demo.gif
Output demo.mp4
Output demo.webp

Set Shell "bash"
Set FontSize 14
Set FontFamily "JetBrains Mono"
Set Width 1200
Set Height 700
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 50ms
Set PlaybackSpeed 1

# ── Setup: put the real binaries on PATH (built ahead of time) ─────────────────
Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Type "clear"
Enter
Show

# ── 1. Version (real binary) ──────────────────────────────────────────────────
Sleep 500ms
Type "korg --version"
Enter
Sleep 1.5s
Ctrl+L

# ── 2. Drive the honest pipeline on a fixture (REAL patch + REAL cargo check) ──
Sleep 400ms
Type 'korg run-once "Fix the add function in src/lib.rs so it adds"'
Enter
Sleep 6s
Ctrl+L

# ── 3. Independently verify the emitted korg-ledger@v1 journal (REAL) ──────────
# The run-once output prints the ledger path; verify it with the standalone
# zero-trust verifier — a green verdict proves the hash-chain + DAG are intact.
Sleep 400ms
Type 'LEDGER=$(korg run-once "Fix the add function in src/lib.rs so it adds" | grep -o "/[^ ]*run-once.jsonl" | head -1)'
Enter
Sleep 4s
Type 'korg-verify "$LEDGER"'
Enter
Sleep 3s
Ctrl+L

# ── 4. Show the real applied change in the fixture worktree (REAL git) ─────────
Sleep 400ms
Type 'cat "$(dirname "$(dirname "$LEDGER")")/src/lib.rs" | head -4'
Enter
Sleep 3s
Ctrl+L

# ── 5. Real reversible rewind of the capability journal ───────────────────────
# Operates on this repo's real .korg/execution_journal.jsonl — a real
# hash-chained truncation + re-seal, not a scripted message.
Sleep 400ms
Type "korg rewind --seq 1"
Enter
Sleep 4s
Ctrl+L

# ── 6. Final card ─────────────────────────────────────────────────────────────
Sleep 400ms
Type "echo ''"
Enter
Type "echo 'Every AI decision. Recorded. Hash-chained. Independently verifiable.'"
Enter
Sleep 600ms
Type "echo ''"
Enter
Type "echo 'A tamper-evident ledger for AI agent cognition — no trust in the tool.'"
Enter
Sleep 600ms
Type "echo ''"
Enter
Type "echo 'cargo install korg   →   github.com/New1Direction/korg'"
Enter
Sleep 4s
