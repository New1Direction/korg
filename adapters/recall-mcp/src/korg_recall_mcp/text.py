"""Turn a korg ledger event into the text string we'll embed for recall.

The shape we expect (from `korg-ingest-claude` and any other adapter that
writes via `make_jsonl_emit`):

    {
      "seq": 42,
      "source_agent": "agent:claude-code#abc",
      "tool_name": "user_prompt" | "llm_inference" | <tool name>,
      "args": {...},
      "result": {...},
      "success": true,
      "duration_ms": 0,
      "triggered_by": 41
    }

The flattening rules below favor signal over noise — we want each
event's text to capture *what it was actually about* in a sentence or
two, so cosine similarity does the heavy lifting on small vectors.
"""

from __future__ import annotations

from typing import Any


_MAX_FIELD_CHARS = 600  # cap any individual field's contribution


def _trim(s: str, n: int = _MAX_FIELD_CHARS) -> str:
    if len(s) <= n:
        return s
    return s[: n - 1] + "…"


def text_for_event(event: dict[str, Any]) -> str:
    """Return a single string describing the event for embedding.

    Returns an empty string if nothing meaningful can be extracted — the
    indexer skips those rather than embedding noise.
    """
    tool = event.get("tool_name", "")
    args = event.get("args") or {}
    result = event.get("result") or {}

    if tool == "user_prompt":
        prompt = args.get("prompt") or args.get("text") or ""
        return _trim(str(prompt))

    if tool == "llm_inference":
        return _trim(str(result.get("text") or ""))

    # Generic tool call: tool_name + key args + a short result preview.
    parts: list[str] = [tool] if tool else []

    # A small set of args usually captures intent; the rest is noise.
    for key in ("file_path", "command", "description", "query", "pattern", "url"):
        v = args.get(key)
        if v:
            parts.append(f"{key}={v}")

    # If no recognized args, fall back to a short JSON-ish dump.
    if len(parts) <= 1 and args:
        try:
            import json as _json

            dumped = _json.dumps(args, default=str)[:300]
            parts.append(dumped)
        except Exception:
            pass

    # Include a short snippet of the result output so e.g. a Read of a file
    # that returned "TODO: rate limiter" is findable by querying "rate limiter".
    output = result.get("output") or result.get("text") or ""
    if isinstance(output, str) and output.strip():
        parts.append(f"→ {output[:300]}")

    text = " ".join(p for p in parts if p)
    return _trim(text)
