// Incremental ledger reader. Same design as the Python EventIndex:
// per-file byte offsets, only load new lines on each refresh, skip
// malformed lines, hold back partial-write lines until the next refresh.

import { promises as fs } from "node:fs";
import { resolve as resolvePath } from "node:path";
import { homedir } from "node:os";
import { textForEvent, type LedgerEvent } from "./text.js";

export interface IndexedEvent {
  sourceFile: string;
  seq: number;
  sourceAgent: string;
  toolName: string;
  args: Record<string, unknown>;
  result: Record<string, unknown>;
  embedText: string;
  embedding: number[] | null;
  triggeredBy: number | null;
  success: boolean;
}

function expanduser(p: string): string {
  if (p.startsWith("~")) return resolvePath(homedir() + p.slice(1));
  return resolvePath(p);
}

export class EventIndex {
  readonly ledgerPaths: string[];
  events: IndexedEvent[] = [];
  private offsets: Map<string, number> = new Map();

  constructor(paths: string[]) {
    this.ledgerPaths = paths.map(expanduser);
  }

  static fromPaths(...paths: string[]): EventIndex {
    return new EventIndex(paths);
  }

  static fromDir(directory: string): EventIndex {
    return new EventIndex([expanduser(directory)]);
  }

  /**
   * Load any new events appended since the last refresh. Returns the
   * number of events added. Idempotent.
   */
  async refresh(): Promise<number> {
    let added = 0;
    const candidates = await this.discoverFiles();

    for (const path of candidates) {
      let stat;
      try {
        stat = await fs.stat(path);
      } catch {
        continue;
      }
      const size = stat.size;
      const offset = this.offsets.get(path) ?? 0;
      if (size <= offset) continue;

      let chunk: string;
      try {
        const buf = await fs.readFile(path, { encoding: "utf8" });
        chunk = buf.slice(offset);
      } catch {
        continue;
      }

      if (!chunk.includes("\n")) continue;
      const lastNl = chunk.lastIndexOf("\n");
      const complete = chunk.slice(0, lastNl);
      const consumed = Buffer.byteLength(complete, "utf8") + 1; // +1 for trailing \n

      const lines = complete.split("\n");
      for (const raw of lines) {
        const line = raw.trim();
        if (!line) continue;

        let obj: LedgerEvent;
        try {
          obj = JSON.parse(line) as LedgerEvent;
        } catch {
          continue;
        }
        if (typeof obj !== "object" || obj === null) continue;

        const embedText = textForEvent(obj);
        if (!embedText) continue;

        this.events.push({
          sourceFile: path,
          seq: typeof obj.seq === "number" ? obj.seq : 0,
          sourceAgent: String(obj.source_agent ?? ""),
          toolName: String(obj.tool_name ?? ""),
          args: (obj.args ?? {}) as Record<string, unknown>,
          result: (obj.result ?? {}) as Record<string, unknown>,
          embedText,
          embedding: null,
          triggeredBy: obj.triggered_by ?? null,
          success: obj.success !== false,
        });
        added += 1;
      }
      this.offsets.set(path, offset + consumed);
    }
    return added;
  }

  private async discoverFiles(): Promise<string[]> {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const p of this.ledgerPaths) {
      let stat;
      try {
        stat = await fs.stat(p);
      } catch {
        continue;
      }
      if (stat.isDirectory()) {
        try {
          const entries = await fs.readdir(p);
          for (const e of entries.sort()) {
            if (e.endsWith(".jsonl")) {
              const full = resolvePath(p, e);
              if (!seen.has(full)) {
                seen.add(full);
                out.push(full);
              }
            }
          }
        } catch {
          continue;
        }
      } else if (stat.isFile()) {
        if (!seen.has(p)) {
          seen.add(p);
          out.push(p);
        }
      }
    }
    return out.sort();
  }

  get length(): number {
    return this.events.length;
  }
}
