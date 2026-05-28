// Recall engine. Substring is always available (no deps). Semantic uses
// @xenova/transformers via lazy dynamic import — package keeps it as an
// optionalDependency so substring-only installs stay tiny.

import { EventIndex, type IndexedEvent } from "./event-index.js";

export type Mode = "auto" | "semantic" | "substring";

export const DEFAULT_MIN_SCORE = 0.30;
export const DEFAULT_TOP_N = 5;
export const DEFAULT_EMBEDDING_MODEL = "Xenova/bge-small-en-v1.5";

export class EmbeddingDependencyMissing extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EmbeddingDependencyMissing";
  }
}

export interface Match {
  event: IndexedEvent;
  score: number;
  via: "semantic" | "substring";
}

interface SearchOptions {
  mode?: Mode;
  topN?: number;
  minScore?: number;
  toolFilter?: readonly string[];
}

// ── Embedder (lazy) ───────────────────────────────────────────────────

class LazyEmbedder {
  private model: unknown = null;
  private modelName: string;

  constructor(modelName: string = DEFAULT_EMBEDDING_MODEL) {
    this.modelName = modelName;
  }

  private async ensureLoaded(): Promise<void> {
    if (this.model !== null) return;
    let pipeline;
    try {
      const mod = await import("@xenova/transformers");
      pipeline = (mod as { pipeline: unknown }).pipeline;
    } catch (e) {
      throw new EmbeddingDependencyMissing(
        "@xenova/transformers is not installed. Run: npm install @xenova/transformers"
      );
    }
    // Suppress transformers.js progress spam to stdout (we need clean stdio for MCP).
    process.env["TRANSFORMERS_VERBOSITY"] = "error";
    this.model = await (pipeline as (task: string, model: string) => Promise<unknown>)(
      "feature-extraction",
      this.modelName
    );
  }

  async embedOne(text: string): Promise<number[]> {
    await this.ensureLoaded();
    const fn = this.model as (
      input: string,
      opts: { pooling: string; normalize: boolean }
    ) => Promise<{ data: Float32Array | number[] }>;
    const out = await fn(text, { pooling: "mean", normalize: true });
    return Array.from(out.data as Float32Array);
  }

  async embedMany(texts: string[]): Promise<number[][]> {
    await this.ensureLoaded();
    const out: number[][] = [];
    // transformers.js doesn't batch as cleanly as fastembed; do sequentially
    // for v1. Slow but correct.
    for (const t of texts) {
      out.push(await this.embedOne(t));
    }
    return out;
  }
}

function cosine(a: readonly number[], b: readonly number[]): number {
  if (a.length === 0 || a.length !== b.length) return 0;
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < a.length; i++) {
    const ax = a[i] as number;
    const bx = b[i] as number;
    dot += ax * bx;
    na += ax * ax;
    nb += bx * bx;
  }
  if (na === 0 || nb === 0) return 0;
  return dot / (Math.sqrt(na) * Math.sqrt(nb));
}

// ── Engine ────────────────────────────────────────────────────────────

export class RecallEngine {
  readonly index: EventIndex;
  private embedder: LazyEmbedder;
  lastMode: "semantic" | "substring" | null = null;

  constructor(index: EventIndex, embedder?: LazyEmbedder) {
    this.index = index;
    this.embedder = embedder ?? new LazyEmbedder();
  }

  async search(query: string, opts: SearchOptions = {}): Promise<Match[]> {
    const mode = opts.mode ?? "auto";
    const topN = opts.topN ?? DEFAULT_TOP_N;
    const minScore = opts.minScore ?? DEFAULT_MIN_SCORE;
    const toolFilter = opts.toolFilter ? new Set(opts.toolFilter) : null;

    if (!query || !query.trim()) return [];

    await this.index.refresh();
    let events = [...this.index.events];
    if (toolFilter) {
      events = events.filter((e) => toolFilter.has(e.toolName));
    }
    if (events.length === 0) return [];

    if (mode === "substring") {
      this.lastMode = "substring";
      return this.searchSubstring(query, events, topN);
    }
    if (mode === "semantic") {
      this.lastMode = "semantic";
      return await this.searchSemantic(query, events, topN, minScore);
    }
    // auto: prefer semantic, fall back to substring on missing dep
    try {
      const results = await this.searchSemantic(query, events, topN, minScore);
      this.lastMode = "semantic";
      return results;
    } catch (e) {
      if (e instanceof EmbeddingDependencyMissing) {
        this.lastMode = "substring";
        return this.searchSubstring(query, events, topN);
      }
      throw e;
    }
  }

  private searchSubstring(query: string, events: IndexedEvent[], topN: number): Match[] {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) return [];
    const out: Match[] = [];
    for (const ev of events) {
      const hay = ev.embedText.toLowerCase();
      if (terms.every((t) => hay.includes(t))) {
        // Score: shorter haystacks get higher scores (more concise = more signal).
        const score = 1.0 - Math.min(0.5, hay.length / 2000.0);
        out.push({ event: ev, score, via: "substring" });
      }
    }
    out.sort((a, b) => b.score - a.score);
    return out.slice(0, topN);
  }

  private async searchSemantic(
    query: string,
    events: IndexedEvent[],
    topN: number,
    minScore: number
  ): Promise<Match[]> {
    const unembedded = events.filter((e) => e.embedding === null);
    if (unembedded.length > 0) {
      const vecs = await this.embedder.embedMany(unembedded.map((e) => e.embedText));
      for (let i = 0; i < unembedded.length; i++) {
        const ev = unembedded[i];
        const vec = vecs[i];
        if (ev && vec) {
          ev.embedding = vec;
        }
      }
    }
    const qvec = await this.embedder.embedOne(query);
    const out: Match[] = [];
    for (const ev of events) {
      if (ev.embedding === null) continue;
      const s = cosine(qvec, ev.embedding);
      if (s >= minScore) {
        out.push({ event: ev, score: s, via: "semantic" });
      }
    }
    out.sort((a, b) => b.score - a.score);
    return out.slice(0, topN);
  }
}
