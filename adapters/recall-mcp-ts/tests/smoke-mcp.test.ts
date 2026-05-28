// End-to-end MCP smoke test: spawn the compiled CLI as a subprocess,
// connect to it via the SDK's stdio Client, do initialize → tools/list →
// tools/call, assert the round-trip works.

import { test } from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

test("end-to-end: SDK client drives the compiled server", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "mcp-smoke-"));
  try {
    const ledger = join(tmp, "events.jsonl");
    await fs.writeFile(ledger,
      JSON.stringify({ seq: 1, source_agent: "agent:claude-code#abc", tool_name: "user_prompt", args: { prompt: "how does the rust borrow checker work" }, result: {}, success: true, duration_ms: 0 }) + "\n" +
      JSON.stringify({ seq: 2, source_agent: "agent:claude-code#abc", tool_name: "llm_inference", args: {}, result: { text: "Rust enforces ownership at compile time." }, success: true, duration_ms: 0, triggered_by: 1 }) + "\n" +
      JSON.stringify({ seq: 3, source_agent: "agent:claude-code#abc", tool_name: "user_prompt", args: { prompt: "css flexbox alignment" }, result: {}, success: true, duration_ms: 0 }) + "\n"
    );

    const transport = new StdioClientTransport({
      command: process.execPath,
      args: [
        "/Users/clubpenguin/Documents/Korg/adapters/recall-mcp-ts/dist/cli.js",
        "--ledger", ledger,
      ],
    });
    const client = new Client(
      { name: "smoke-test", version: "0.0.0" },
      { capabilities: {} }
    );
    await client.connect(transport);

    // tools/list
    const toolsResp = await client.listTools();
    assert.equal(toolsResp.tools.length, 1);
    assert.equal(toolsResp.tools[0]!.name, "recall");
    assert.equal((toolsResp.tools[0]!.inputSchema as { type: string }).type, "object");

    // tools/call
    const callResp = await client.callTool({
      name: "recall",
      arguments: { query: "rust", mode: "substring", top_n: 5 },
    });
    const content = callResp.content as { type: string; text: string }[];
    assert.equal(content[0]!.type, "text");
    const text = content[0]!.text;
    assert.match(text, /\[recall · substring\]/);
    assert.match(text, /seq=1/); // the rust borrow checker prompt

    await client.close();
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
