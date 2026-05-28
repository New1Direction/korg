import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { mkdtempSync, rmSync } from "node:fs";
import { EventIndex } from "../src/event-index.js";

let tmp: string;

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "ti-test-"));
});
afterEach(() => {
  rmSync(tmp, { recursive: true, force: true });
});

async function writeEvent(
  path: string,
  seq: number,
  toolName: string,
  args: Record<string, unknown>,
  extras: { result?: Record<string, unknown>; triggered_by?: number } = {}
): Promise<void> {
  const record = {
    seq,
    source_agent: "agent:test",
    tool_name: toolName,
    args,
    result: extras.result ?? {},
    success: true,
    duration_ms: 0,
    ...(extras.triggered_by !== undefined ? { triggered_by: extras.triggered_by } : {}),
  };
  await fs.appendFile(path, JSON.stringify(record) + "\n");
}

test("index reads basic jsonl", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "hi" });
  await writeEvent(p, 2, "llm_inference", {}, { result: { text: "hello" } });

  const idx = EventIndex.fromPaths(p);
  const n = await idx.refresh();
  assert.equal(n, 2);
  assert.equal(idx.length, 2);
  assert.equal(idx.events[0]!.toolName, "user_prompt");
  assert.equal(idx.events[0]!.embedText, "hi");
});

test("incremental refresh skips already-loaded", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "first" });

  const idx = EventIndex.fromPaths(p);
  assert.equal(await idx.refresh(), 1);
  assert.equal(await idx.refresh(), 0); // no new

  await writeEvent(p, 2, "user_prompt", { prompt: "second" });
  assert.equal(await idx.refresh(), 1);
  assert.equal(idx.length, 2);
});

test("skips malformed lines", async () => {
  const p = join(tmp, "ledger.jsonl");
  await fs.writeFile(p,
    JSON.stringify({ seq: 1, tool_name: "user_prompt", args: { prompt: "ok" } }) + "\n" +
    "not json\n\n" +
    JSON.stringify({ seq: 2, tool_name: "user_prompt", args: { prompt: "fine" } }) + "\n"
  );

  const idx = EventIndex.fromPaths(p);
  await idx.refresh();
  assert.equal(idx.length, 2);
  assert.equal((idx.events[0]!.args as { prompt: string }).prompt, "ok");
  assert.equal((idx.events[1]!.args as { prompt: string }).prompt, "fine");
});

test("skips events with no embed text", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "llm_inference", {}, { result: { completion_tokens: 1 } });
  await writeEvent(p, 2, "user_prompt", { prompt: "" });
  await writeEvent(p, 3, "user_prompt", { prompt: "real one" });

  const idx = EventIndex.fromPaths(p);
  await idx.refresh();
  assert.equal(idx.length, 1);
  assert.equal((idx.events[0]!.args as { prompt: string }).prompt, "real one");
});

test("partial mid-write line held back", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "complete" });
  // Append partial line (no trailing newline)
  await fs.appendFile(p, '{"seq":2,"tool_name":"user_prompt","args":{"prompt":"partial');

  const idx = EventIndex.fromPaths(p);
  const n1 = await idx.refresh();
  assert.equal(n1, 1); // only the complete line

  // Finish the partial line
  await fs.appendFile(p, '"}}\n');
  const n2 = await idx.refresh();
  assert.equal(n2, 1);
  assert.equal(idx.length, 2);
});

test("reads from directory of .jsonl files", async () => {
  const f1 = join(tmp, "session-a.jsonl");
  const f2 = join(tmp, "session-b.jsonl");
  await writeEvent(f1, 1, "user_prompt", { prompt: "from a" });
  await writeEvent(f2, 1, "user_prompt", { prompt: "from b" });

  const idx = EventIndex.fromDir(tmp);
  await idx.refresh();
  const prompts = idx.events
    .map((e) => (e.args as { prompt: string }).prompt)
    .sort();
  assert.deepEqual(prompts, ["from a", "from b"]);
});

test("picks up new file in watched directory", async () => {
  const f1 = join(tmp, "session-a.jsonl");
  await writeEvent(f1, 1, "user_prompt", { prompt: "from a" });

  const idx = new EventIndex([tmp]);
  await idx.refresh();
  assert.equal(idx.length, 1);

  const f2 = join(tmp, "session-b.jsonl");
  await writeEvent(f2, 1, "user_prompt", { prompt: "from b" });
  const n = await idx.refresh();
  assert.equal(n, 1);
  assert.equal(idx.length, 2);
});

test("handles missing paths gracefully", async () => {
  const idx = EventIndex.fromPaths(join(tmp, "does-not-exist.jsonl"));
  assert.equal(await idx.refresh(), 0);
  assert.equal(idx.length, 0);
});

test("preserves triggered_by and success", async () => {
  const p = join(tmp, "ledger.jsonl");
  await writeEvent(p, 1, "user_prompt", { prompt: "go" });
  await writeEvent(p, 2, "Bash", { command: "false" }, {
    result: { output: "exit 1" },
    triggered_by: 1,
  });
  const idx = EventIndex.fromPaths(p);
  await idx.refresh();
  const bash = idx.events.find((e) => e.toolName === "Bash")!;
  assert.equal(bash.triggeredBy, 1);
});
