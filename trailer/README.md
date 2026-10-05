# KORG launch trailer

A 60-second launch film, built as code. `index.html` draws any frame of the film from a single
`renderAt(t)` function; `audio.mjs` synthesises the soundtrack (no samples, no dependencies);
`render.mjs` steps the page at 30 fps and encodes the video.

| File | What it is |
|---|---|
| `out/korg-trailer.mp4` | The full 60 s trailer, 1280×720, 30 fps, with sound |
| `out/korg-rewind-8s.mp4` | The 8 s REWIND → BRANCH clip (0:26–0:34), cut for X |
| `index.html` | The film. Open it in a browser and press play, or add `?t=27.5` to freeze a frame |
| `audio.mjs` | Sound design, cue by cue, with the three hard silences (0:11, 0:30, 0:50) |
| `render.mjs` | Frame capture + ffmpeg mux, and the 8 s clip cut |

## Rebuild

```bash
node trailer/audio.mjs      # -> trailer/out/korg-trailer.wav
node trailer/render.mjs     # -> trailer/out/korg-trailer.mp4 + korg-rewind-8s.mp4  (~5 min)
FROM=26 TO=34 NAME=preview node trailer/render.mjs      # render just a range
```

Needs Node 18+, `playwright` with Chromium, and `ffmpeg`. `CAPTIONS=0` (or `?captions=0` in the browser) removes the voiceover captions.

## What's on screen is real

The trailer tells the story of the actual `korg demo` (README → "Try the Time-Travel Demo"):

- **seq 390–393** – prompt, Read, a wrong Edit that leaves `return a + b`, `pytest` fails
- **rewind to seq 391**, branch, `return a - b`, `2 passed`
- The six hashes are the real `entry_hash` values of `spec/korg-ledger-v1/web/samples/session.jsonl`
- The `✓ journal VALID — 6 events, hash-chain + DAG intact` line is the real `korg-verify` output
- The code texture in the background is `crates/korg-ledger/src/lib.rs` (the chain verifier)
- "3 agents, one format" is the N=3 adapter table (Claude Code, Codex CLI, Grok Heavy)

## Timeline

| Time | Beat |
|---|---|
| 0:00 | GRONK wordmark → distortion → fragments into hex data |
| 0:03 | Fragments converge on KORG, hard black, impact, `AI AGENT MEMORY + VERIFICATION` |
| 0:06 | The agent at work: terminal / editor / browser cuts, `ACTION` ×4, then `ERROR`, then silence |
| 0:12 | Dive into the failing line → the agent's history as a 3D event graph |
| 0:18 | The error event opens into the hash-linked chain; `REMEMBER EVERYTHING.` |
| 0:26 | **`REWIND`** – playhead scrubs back through the ledger, red events un-execute, stop at seq 391 |
| 0:32 | **`BRANCH`** – the second future forks from seq 391 |
| 0:34 | Branch executes, `✓ PASSED`, camera pulls back to reveal the whole branching history |
| 0:43 | Machinery cuts: event graph · REPLAY · VERIFY · agent session · REWIND · BRANCH |
| 0:50 | `AI CAN MAKE MISTAKES.` / `AI DOESN'T HAVE TO FORGET THEM.` |
| 0:56 | KORG, `A MEMORY AND VERIFICATION LAYER FOR AI AGENTS.`, then `REPO.ING` |

## Voiceover

The film ships with the voiceover as burned-in captions (there's no recorded voice). To add one, record
these lines against the timestamps and mix over `out/korg-trailer.wav`, then render without captions (`CAPTIONS=0 node trailer/render.mjs`):

- 0:13 "AI agents are going to make mistakes." · 0:15.6 "The problem is…" · 0:17 "…what happens next?"
- 0:19 "What if you could see exactly what happened?"
- 0:36.7 "Don't just know what an agent did." · 0:39.3 "Know why it happened." · 0:41.5 "And change what happens next."

The GRONK wordmark is plain type, not any company's logo.
