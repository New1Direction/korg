# Phase 1 — Plan 6: Readers Unification + Cross-Producer CI Gate + Honest Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close out Phase 1 — make `recall-mcp` read the canonical per-session ledgers (so search keeps working after the format switch), enforce cross-language conformance on every push with a CI gate, and fix the docs that still call the flat file a ledger.

**Architecture:** A `_normalize_record` accessor lets `recall-mcp` read both legacy-flat and canonical `JournalEvent` records (normalizing canonical's nested `event.*`/`metadata.*` into the flat shape the embedder already expects), and the default ledger source becomes `~/.korg/sessions/`. A new `conformance` CI job runs the Python and JS spec oracles plus the stdlib-only adapter suites. Targeted doc edits describe the one verifiable capture path.

**Tech Stack:** Python 3.9+, stdlib only; GitHub Actions; the `korg_recall_mcp` package; `pytest`.

---

## File Structure

```
adapters/recall-mcp/src/korg_recall_mcp/index.py      # MODIFY: _normalize_record (flat + canonical)
adapters/recall-mcp/src/korg_recall_mcp/__main__.py   # MODIFY: default ledger → ~/.korg/sessions/
adapters/recall-mcp/tests/test_index.py               # MODIFY: canonical-record test
.github/workflows/ci.yml                              # MODIFY: add `conformance` job
adapters/claude-code/README.md                        # MODIFY: honest capture framing
INSTALLATION_GUIDE.md                                 # MODIFY: one verifiable path
```

---

### Task 1: `recall-mcp` reads canonical per-session ledgers

**Files:**
- Modify: `adapters/recall-mcp/src/korg_recall_mcp/index.py`
- Modify: `adapters/recall-mcp/src/korg_recall_mcp/__main__.py`
- Test: `adapters/recall-mcp/tests/test_index.py`

- [ ] **Step 1: Write the failing test** (append to `test_index.py`)

```python
def test_reads_canonical_journalevent_records(tmp_path):
    from korg_recall_mcp.index import EventIndex
    led = tmp_path / "sess.jsonl"
    # one canonical korg-ledger@v1 JournalEvent line (nested event/metadata)
    led.write_text(json.dumps({
        "schema_version": "1.0", "seq_id": 7,
        "metadata": {"triggered_by": 6, "actor_id": "korg:claude-hook"},
        "event": {"event_type": "AgentToolCall", "source_agent": "agent:claude-code#s1",
                  "tool_name": "Read", "args": {"file_path": "rate_limiter.py"},
                  "result": {"output": "TODO: token bucket"}, "success": True, "duration_ms": 5},
        "prev_hash": "0" * 64, "entry_hash": "abc",
    }) + "\n")
    idx = EventIndex.from_paths(led)
    assert idx.refresh() == 1
    e = idx.events[0]
    assert e.seq == 7
    assert e.source_agent == "agent:claude-code#s1"
    assert e.tool_name == "Read"
    assert e.args == {"file_path": "rate_limiter.py"}
    assert e.triggered_by == 6
    assert "rate_limiter.py" in e.embed_text  # searchable via the nested args
```

(Ensure `import json` is present at the top of `test_index.py`.)

- [ ] **Step 2: Run it and watch it fail**

Run: `PYTHONPATH=adapters/recall-mcp/src python3 -m pytest adapters/recall-mcp/tests/test_index.py::test_reads_canonical_journalevent_records -q`
Expected: FAIL — the flat parser reads top-level `tool_name` (absent in canonical), so `tool_name`/`args` are empty and `embed_text` won't contain `rate_limiter.py`.

- [ ] **Step 3: Add `_normalize_record` and use it in `refresh`**

In `index.py`, add this module-level function above `EventIndex`:

```python
def _normalize_record(obj: dict) -> dict:
    """Return a flat {seq, source_agent, tool_name, args, result, triggered_by, success}
    from EITHER a legacy flat record OR a canonical korg-ledger@v1 JournalEvent."""
    ev = obj.get("event")
    if isinstance(ev, dict) and "tool_name" in ev:  # canonical JournalEvent
        meta = obj.get("metadata") or {}
        return {
            "seq": obj.get("seq_id", 0),
            "source_agent": ev.get("source_agent", ""),
            "tool_name": ev.get("tool_name", ""),
            "args": ev.get("args") or {},
            "result": ev.get("result") or {},
            "triggered_by": meta.get("triggered_by"),
            "success": ev.get("success", True),
        }
    return {  # legacy flat
        "seq": obj.get("seq", 0),
        "source_agent": obj.get("source_agent", ""),
        "tool_name": obj.get("tool_name", ""),
        "args": obj.get("args") or {},
        "result": obj.get("result") or {},
        "triggered_by": obj.get("triggered_by"),
        "success": obj.get("success", True),
    }
```

Then in `refresh()`, replace the block from `embed_text = text_for_event(obj)` through the `IndexedEvent(...)` append with:

```python
                rec = _normalize_record(obj)
                embed_text = text_for_event(rec)
                if not embed_text:
                    continue
                self.events.append(
                    IndexedEvent(
                        source_file=key,
                        seq=int(rec["seq"] or 0),
                        source_agent=str(rec["source_agent"]),
                        tool_name=str(rec["tool_name"]),
                        args=dict(rec["args"]),
                        result=dict(rec["result"]),
                        embed_text=embed_text,
                        triggered_by=rec["triggered_by"],
                        success=bool(rec["success"]),
                    )
                )
                added += 1
```

- [ ] **Step 4: Default the ledger source to the per-session dir**

In `__main__.py`, change the default:

```python
DEFAULT_LEDGER = Path.home() / ".korg" / "sessions"
```

and update the `--ledger` help text to: `"Path to a ledger .jsonl file OR a directory of per-session ledgers (default ~/.korg/sessions/). Repeatable."`

- [ ] **Step 5: Run the test + the full recall index suite**

Run: `PYTHONPATH=adapters/recall-mcp/src python3 -m pytest adapters/recall-mcp/tests/test_index.py -q`
Expected: PASS — the new canonical test plus the existing flat-format tests (the legacy branch of `_normalize_record` preserves them).

- [ ] **Step 6: Commit**

```bash
git add adapters/recall-mcp/src/korg_recall_mcp/index.py adapters/recall-mcp/src/korg_recall_mcp/__main__.py adapters/recall-mcp/tests/test_index.py
git commit -m "feat(recall-mcp): read canonical per-session ledgers; default to ~/.korg/sessions/"
```

---

### Task 2: Cross-language conformance CI gate

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add a `conformance` job** (append under `jobs:`)

```yaml
  conformance:
    name: Cross-language ledger conformance
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: "3.11"

      - uses: actions/setup-node@v4
        with:
          node-version: "20"

      - name: Python spec oracle reproduces the frozen vectors
        run: python3 spec/korg-ledger-v1/conformance.py

      - name: JS spec oracle reproduces the frozen vectors
        run: node spec/korg-ledger-v1/js/conformance.mjs

      - name: Install pytest
        run: python3 -m pip install --quiet pytest

      - name: Producer + capture + setup + reader test suites
        run: |
          PYTHONPATH="adapters/korg-ledger-py/src:adapters/claude-code/src:adapters/korg-setup/src:adapters/recall-mcp/src" \
            python3 -m pytest -q \
              adapters/korg-ledger-py/tests \
              adapters/claude-code/tests \
              adapters/korg-setup/tests \
              adapters/recall-mcp/tests/test_index.py \
              adapters/recall-mcp/tests/test_text.py
```

(The Rust frozen-vector conformance is already gated by the existing `build` job's `cargo test`. `recall-mcp`'s search tests are excluded here because they pull optional embedding deps; `test_index`/`test_text` are stdlib-only and gate the canonical reader.)

- [ ] **Step 2: Verify the job's commands locally** (the real proof the gate is correct)

Run exactly what the job runs:
```bash
python3 spec/korg-ledger-v1/conformance.py
node spec/korg-ledger-v1/js/conformance.mjs
PYTHONPATH="adapters/korg-ledger-py/src:adapters/claude-code/src:adapters/korg-setup/src:adapters/recall-mcp/src" \
  python3 -m pytest -q adapters/korg-ledger-py/tests adapters/claude-code/tests adapters/korg-setup/tests \
  adapters/recall-mcp/tests/test_index.py adapters/recall-mcp/tests/test_text.py
```
Expected: both oracles print `PASS`; the pytest run is all-green.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: gate cross-language korg-ledger conformance + adapter suites on every push"
```

---

### Task 3: Honest docs — one verifiable capture path

**Files:**
- Modify: `adapters/claude-code/README.md`
- Modify: `INSTALLATION_GUIDE.md`

- [ ] **Step 1: Rewrite the claude-code adapter README capture section**

Replace any text describing the output as a flat `claude-events.jsonl` "ledger" with an accurate description. Add/replace the top section with:

```markdown
## What it produces

Each Claude Code session is captured into a **verifiable per-session ledger** at
`~/.korg/sessions/<session_id>.jsonl` — a `korg-ledger@v1` hash-chain you can
independently verify with `korg-verify` (or the JS verifier in a browser). The
capture is passive and zero-config once `korg-setup` registers the `korg-hook`
PostToolUse/Stop hook; `korg-backfill` re-derives the same verifiable ledgers
for every historical session.

> The earlier flat `~/.korg/claude-events.jsonl` (one un-chained `{seq, ...}`
> line per event) is **legacy** — it carried no hash chain and is superseded by
> the per-session verifiable ledgers. `korg-backfill --migrate-flat` converts it.
```

- [ ] **Step 2: Update `INSTALLATION_GUIDE.md` capture instructions**

Find the section that tells users to run the tail daemon / points at `claude-events.jsonl`, and replace it with the hook-first flow:

```markdown
### Turn on verifiable capture (one command)

```bash
korg-setup            # registers the korg-hook capture hook in ~/.claude/settings.json
korg-backfill         # (optional) retroactively capture your existing sessions
```

New Claude Code sessions are now recorded to verifiable per-session ledgers under
`~/.korg/sessions/`. Verify any of them:

```bash
korg-verify ~/.korg/sessions/<session_id>.jsonl
```

The always-on launchd tail daemon is still available behind `korg-setup --daemon`
(macOS), but it is no longer required — the hook is cross-platform and on by default.
```

- [ ] **Step 3: Sanity-check the docs render** (no code to run; just read them back)

Run: `grep -n "sessions/" adapters/claude-code/README.md INSTALLATION_GUIDE.md`
Expected: the new per-session paths are present; no remaining text calls the flat file "the ledger."

- [ ] **Step 4: Commit**

```bash
git add adapters/claude-code/README.md INSTALLATION_GUIDE.md
git commit -m "docs: describe the one verifiable per-session capture path (flat file is legacy)"
```

---

## Self-Review

**1. Spec coverage (§4.8, §4.9, §4.10):** `recall-mcp` reads canonical per-session ledgers via a thin accessor that also preserves the flat format ✓ (Task 1); default source is `~/.korg/sessions/` ✓ (Task 1 Step 4); `korg-verify` already reads the canonical JSONL (no change needed — proven by the Plan 1/3 oracle E2E); cross-producer conformance gated in CI across Python + JS + the adapter suites, with Rust already gated by the existing `cargo test` ✓ (Task 2); honest docs that stop calling the flat file a ledger ✓ (Task 3).

**2. Placeholder scan:** No TBD/TODO; complete code/YAML/markdown in every step; exact commands + expected output.

**3. Type/name consistency:** `_normalize_record` returns exactly the keys the existing `text_for_event` and `IndexedEvent` consume (`seq, source_agent, tool_name, args, result, triggered_by, success`); the canonical-branch mapping (`seq_id`, `event.*`, `metadata.triggered_by`) matches the `JournalEvent` shape produced by `korg-ledger-py` and the Rust crate. The CI `PYTHONPATH` and test paths match the repo layout used throughout Plans 1–5. No gaps found.
