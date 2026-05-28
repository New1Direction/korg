import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { EventIndex } from "../src/event-index.js";
import {
  EmbeddingDependencyMissing,
  RecallEngine,
} from "../src/search.js";

let tmp: string;

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "rs-test-"));
});
afterEach(() => {
  rmSync(tmp, { recursive: true, force: true });
});

async function writeEvent(path: string, seq: number, toolName: string, args: Record<string, unknown>, result: Record<string, unknown> = {}): Promise<void> {
  await fs.appendFile(path, JSON.stringify({
    seq, source_agent: "agent:test", tool_name: toolName,
    args, result, success: true, duration_ms: 0,
  }) + "\n");
}

async function seedIndex(): Promise<EventIndex> {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "how does the rust borrow checker prevent data races" });
  await writeEvent(p, 2, "llm_inference", {}, { text: "Rust enforces ownership at compile time." });
  await writeEvent(p, 3, "user_prompt", { prompt: "best css flexbox alignment tricks" });
  await writeEvent(p, 4, "llm_inference", {}, { text: "Use align-items: center for vertical centering." });
  await writeEvent(p, 5, "user_prompt", { prompt: "ownership and lifetimes in rust" });
  await writeEvent(p, 6, "Bash", { command: "cargo test", description: "run rust tests" }, { output: "5 passed" });
  const idx = EventIndex.fromPaths(p);
  await idx.refresh();
  return idx;
}

// ── Substring ─────────────────────────────────────────────────────────

test("substring returns only events matching ALL terms", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("rust ownership", { mode: "substring", topN: 10 });
  const foundSeqs = matches.map((m) => m.event.seq).sort();
  assert.ok(foundSeqs.includes(5)); // "ownership and lifetimes in rust" matches both
  assert.ok(!foundSeqs.includes(1)); // "rust borrow checker..." has rust but no ownership
});

test("substring is case insensitive", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("RUST", { mode: "substring", topN: 10 });
  assert.ok(matches.length >= 2);
});

test("substring returns empty when no match", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("quantum chromodynamics", { mode: "substring", topN: 10 });
  assert.equal(matches.length, 0);
});

test("substring respects topN", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("rust", { mode: "substring", topN: 2 });
  assert.ok(matches.length <= 2);
});

test("via field is set to substring", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("rust", { mode: "substring", topN: 5 });
  assert.ok(matches.every((m) => m.via === "substring"));
});

test("lastMode tracked", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  await eng.search("rust", { mode: "substring" });
  assert.equal(eng.lastMode, "substring");
});

test("empty query returns empty", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  assert.equal((await eng.search("", { mode: "substring" })).length, 0);
  assert.equal((await eng.search("   ", { mode: "substring" })).length, 0);
});

test("toolFilter restricts results", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  const matches = await eng.search("rust", {
    mode: "substring",
    topN: 10,
    toolFilter: ["user_prompt"],
  });
  assert.ok(matches.every((m) => m.event.toolName === "user_prompt"));
});

test("refresh is called automatically on search", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "rust borrow checker" });
  const idx = EventIndex.fromPaths(p);
  const eng = new RecallEngine(idx);

  const m1 = await eng.search("rust", { mode: "substring", topN: 5 });
  assert.equal(m1.length, 1);

  await writeEvent(p, 2, "user_prompt", { prompt: "rust lifetimes" });
  const m2 = await eng.search("rust", { mode: "substring", topN: 5 });
  assert.equal(m2.length, 2);
});

// ── Semantic fallback (no fastembed equivalent installed) ─────────────

test("auto mode falls back to substring when embedder is missing", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);

  // Inject a stub embedder that always reports missing dep
  // (since @xenova/transformers is an optionalDependency, it may or may
  // not be present in the test env — force the fallback path).
  (eng as unknown as { embedder: unknown }).embedder = {
    async embedOne(): Promise<number[]> {
      throw new EmbeddingDependencyMissing("simulated missing");
    },
    async embedMany(): Promise<number[][]> {
      throw new EmbeddingDependencyMissing("simulated missing");
    },
  };

  const matches = await eng.search("rust", { mode: "auto" });
  assert.equal(eng.lastMode, "substring");
  assert.ok(matches.some((m) => m.event.embedText.toLowerCase().includes("rust")));
});

test("explicit semantic mode raises without embedder", async () => {
  const idx = await seedIndex();
  const eng = new RecallEngine(idx);
  (eng as unknown as { embedder: unknown }).embedder = {
    async embedOne(): Promise<number[]> {
      throw new EmbeddingDependencyMissing("simulated missing");
    },
    async embedMany(): Promise<number[][]> {
      throw new EmbeddingDependencyMissing("simulated missing");
    },
  };
  await assert.rejects(() => eng.search("anything", { mode: "semantic" }), EmbeddingDependencyMissing);
});
