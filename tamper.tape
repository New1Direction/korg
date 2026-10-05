# korg tamper demo — the chain catching a rewritten event.
#
# Runs the REAL `korg-verify` binary against a real korg-ledger@v1 journal
# (spec/korg-ledger-v1/web/samples/session.jsonl, a 6-event Claude Code
# session). One byte of history is edited with `sed`; the verifier names the
# exact event that was tampered with and exits non-zero. Nothing is scripted.
#
# Regenerate:  cargo build --release -p korg-verify && vhs tamper.tape

Output tamper.gif

Set Shell "bash"
Set FontSize 15
Set FontFamily "DejaVu Sans Mono"
Set Width 1200
Set Height 480
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 40ms

# ── Setup: verifier on PATH, work on a scratch copy of the sample ledger ──────
Hide
Type "export PATH=$PWD/target/release:$PATH PS1='$ '"
Enter
Type "D=$(mktemp -d) && cp spec/korg-ledger-v1/web/samples/session.jsonl $D/ && cd $D && clear"
Enter
Show

# ── 1. An agent session, recorded as a hash-chained ledger ─────────────────────
Sleep 500ms
Type `jq -c '[.seq_id, .event.tool_name, .event.args.new_string // .event.args.command // ""]' session.jsonl`
Enter
Sleep 2.5s

# ── 2. Verify it — no trust in whoever produced it ────────────────────────────
Type "korg-verify session.jsonl"
Enter
Sleep 2s

# ── 3. Quietly rewrite history: make event 4 look like the buggy edit ──────────
Type "sed -i.bak '4s/return a - b/return a + b/' session.jsonl"
Enter
Sleep 1s

# ── 4. Verify again — the chain names the exact event that was changed ────────
Type "korg-verify session.jsonl; echo exit=$?"
Enter
Sleep 4s
