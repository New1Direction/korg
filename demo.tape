# korg demo tape — scripted simulation via demo-sim.sh
# Run with: docker run --rm -v $PWD:/vhs ghcr.io/charmbracelet/vhs demo.tape

Output demo.gif
Output demo.mp4
Output demo.webp

Set Shell "bash"
Set FontSize 14
Set FontFamily "JetBrains Mono"
Set Width 1200
Set Height 600
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 55ms
Set PlaybackSpeed 1

# ── Setup ─────────────────────────────────────────────────────────────────────
Hide
Type "chmod +x /vhs/demo-sim.sh && alias korg-sim='/vhs/demo-sim.sh'"
Enter
Show

# ── 1. Version ────────────────────────────────────────────────────────────────
Sleep 500ms
Type "korg --version"
Enter
Sleep 200ms
Type "/vhs/demo-sim.sh version"
Enter
Sleep 1.5s
Ctrl+L

# ── 2. Campaign run ───────────────────────────────────────────────────────────
Sleep 400ms
Type 'korg "Fix the authentication bug in src/auth.rs" --headless'
Enter
Sleep 400ms
Type '/vhs/demo-sim.sh run "Fix the authentication bug in src/auth.rs"'
Enter
Sleep 8s
Ctrl+L

# ── 3. Rewind ─────────────────────────────────────────────────────────────────
Sleep 400ms
Type "korg rewind --seq 3"
Enter
Sleep 400ms
Type "/vhs/demo-sim.sh rewind 3"
Enter
Sleep 3s

# ── 4. Git log shows snapped-back state ───────────────────────────────────────
Sleep 400ms
Type "git log --oneline -4"
Enter
Sleep 2s
Ctrl+L

# ── 5. Fork with new strategy ─────────────────────────────────────────────────
Sleep 400ms
Type 'korg "Fix auth using stateless JWT tokens" --headless'
Enter
Sleep 400ms
Type '/vhs/demo-sim.sh fork "Fix auth using stateless JWT tokens"'
Enter
Sleep 6s
Ctrl+L

# ── 6. Final card ─────────────────────────────────────────────────────────────
Sleep 400ms
Type "echo ''"
Enter
Type "echo 'Every AI decision. Logged. Causally ordered. Reversible.'"
Enter
Sleep 600ms
Type "echo ''"
Enter
Type "echo 'The first deterministic cognitive runtime.'"
Enter
Sleep 600ms
Type "echo ''"
Enter
Type "echo 'cargo install korg   →   github.com/New1Direction/korg'"
Enter
Sleep 4s
