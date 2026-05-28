import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { EventIndex } from "../src/event-index.js";
import { RecallEngine } from "../src/search.js";
import {
  formatMatchesForLlm,
  handleRecallCall,
  type RecallArguments,
} from "../src/server.js";

let tmp: string;

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "sv-test-"));
});
afterEach(() => {
  rmSync(tmp, { recursive: true, force: true });
});

async function seedLedger(): Promise<string> {
  const p = join(tmp, "ledger.jsonl");
  await fs.writeFile(p,
    JSON.stringify({ seq: 1, source_agent: "agent:claude-code#abc", tool_name: "user_prompt", args: { prompt: "rust borrow checker question" }, result: {}, success: true, duration_ms: 0 }) + "\n" +
    JSON.stringify({ seq: 2, source_agent: "agent:claude-code#abc", tool_name: "llm_inference", args: {}, result: { text: "rust enforces ownership" }, success: true, duration_ms: 0 }) + "\n" +
    JSON.stringify({ seq: 3, source_agent: "agent:claude-code#abc", tool_name: "user_prompt", args: { prompt: "css flexbox tips" }, result: {}, success: true, duration_ms: 0 }) + "\n"
  );
  return p;
}

async function newEngine(): Promise<RecallEngine> {
  const p = await seedLedger();
  const idx = EventIndex.fromPaths(p);
  return new RecallEngine(idx);
}

// ── handleRecallCall ──────────────────────────────────────────────────

test("returns matches as text content", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, { query: "rust", mode: "substring" });
  assert.equal(result.content[0]!.type, "text");
  const text = result.content[0]!.text;
  assert.ok(text.includes("recall"));
  assert.ok(/rust|borrow/i.test(text));
});

test("empty query returns isError", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, { query: "" });
  assert.equal(result.isError, true);
  assert.match(result.content[0]!.text, /empty query/);
});

test("returns no-match message when nothing relevant", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, { query: "quantum chromodynamics", mode: "substring" });
  assert.match(result.content[0]!.text, /no relevant matches/);
});

test("tool_filter restricts results", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, {
    query: "rust", mode: "substring",
    tool_filter: ["user_prompt"],
  });
  const text = result.content[0]!.text;
  assert.ok(text.includes("tool=user_prompt"));
  assert.ok(!text.includes("tool=llm_inference"));
});

test("invalid mode falls back to auto", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, { query: "rust", mode: "garbage" as unknown });
  // Auto with no semantic embedder available falls to substring, which finds rust.
  assert.ok(/recall/.test(result.content[0]!.text));
});

test("non-string query is stringified", async () => {
  const eng = await newEngine();
  const result = await handleRecallCall(eng, { query: 42 as unknown as RecallArguments["query"] });
  // "42" obviously matches nothing in the seeded ledger; should at least not throw.
  assert.match(result.content[0]!.text, /\[recall/);
});

// ── format ────────────────────────────────────────────────────────────

test("formatMatchesForLlm handles empty", () => {
  assert.match(formatMatchesForLlm([], "semantic"), /no relevant matches/);
});

test("formatMatchesForLlm includes seq, score, tool", () => {
  const fakeMatch = {
    event: {
      sourceFile: "/tmp/x.jsonl",
      seq: 42,
      sourceAgent: "agent:test#abc",
      toolName: "user_prompt",
      args: {},
      result: {},
      embedText: "test prompt",
      embedding: null,
      triggeredBy: null,
      success: true,
    },
    score: 0.87,
    via: "semantic" as const,
  };
  const out = formatMatchesForLlm([fakeMatch], "semantic");
  assert.ok(out.includes("seq=42"));
  assert.ok(out.includes("score=0.87"));
  assert.ok(out.includes("tool=user_prompt"));
});
