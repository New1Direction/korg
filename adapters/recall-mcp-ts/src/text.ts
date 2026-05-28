// Turn a korg ledger event into the text string we embed/search.
//
// Mirrors the Python reference implementation at
// adapters/recall-mcp/src/korg_recall_mcp/text.py — same per-event-type
// rules so the two implementations rank the same ledger identically.

export interface LedgerEvent {
  seq?: number;
  source_agent?: string;
  tool_name?: string;
  args?: Record<string, unknown> | null;
  result?: Record<string, unknown> | null;
  success?: boolean;
  duration_ms?: number;
  triggered_by?: number | null;
}

const MAX_FIELD_CHARS = 600;

function trim(s: string, n: number = MAX_FIELD_CHARS): string {
  if (s.length <= n) return s;
  return s.slice(0, n - 1) + "…";
}

function asString(v: unknown): string {
  if (typeof v === "string") return v;
  if (v == null) return "";
  return String(v);
}

/**
 * Return a single string describing the event for embedding / substring search.
 * Empty string when nothing meaningful can be extracted — the indexer
 * skips those rather than embedding noise.
 */
export function textForEvent(event: LedgerEvent): string {
  const tool = event.tool_name ?? "";
  const args = (event.args ?? {}) as Record<string, unknown>;
  const result = (event.result ?? {}) as Record<string, unknown>;

  if (tool === "user_prompt") {
    const prompt = asString(args["prompt"] ?? args["text"] ?? "");
    return trim(prompt);
  }

  if (tool === "llm_inference") {
    return trim(asString(result["text"] ?? ""));
  }

  // Generic tool call: tool_name + a few key args + a result preview.
  const parts: string[] = [];
  if (tool) parts.push(tool);

  for (const key of ["file_path", "command", "description", "query", "pattern", "url"]) {
    const v = args[key];
    if (v != null && v !== "") {
      parts.push(`${key}=${asString(v)}`);
    }
  }

  // Fallback for unknown args: a short JSON dump.
  if (parts.length <= 1 && Object.keys(args).length > 0) {
    try {
      parts.push(JSON.stringify(args).slice(0, 300));
    } catch {
      // ignore
    }
  }

  // Result snippet so e.g. a Read of a file that returned "TODO: rate limiter"
  // is findable by querying "rate limiter".
  const output = result["output"] ?? result["text"] ?? "";
  const outStr = asString(output);
  if (outStr.trim().length > 0) {
    parts.push(`→ ${outStr.slice(0, 300)}`);
  }

  const text = parts.filter((p) => p.length > 0).join(" ");
  return trim(text);
}
