// Unit tests for the MCP server wiring + an end-to-end SDK-client smoke test.

import { test } from "node:test";
import assert from "node:assert/strict";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { discover } from "../src/discovery.js";
import { Policy } from "../src/safety.js";
import { buildToolsList } from "../src/server.js";
import { makeFixtureBinary } from "./conftest.js";

// ── buildToolsList ────────────────────────────────────────────────────

test("buildToolsList exposes all callables as MCP tools", async () => {
  const f = await makeFixtureBinary();
  try {
    const binary = await discover(f.binPath);
    const tools = buildToolsList(binary);
    const names = new Set(tools.map((t) => t.name));
    assert.deepEqual(
      [...names].sort(),
      ["fixture-bin.echo", "fixture-bin.fail", "fixture-bin.shell", "fixture-bin.write"]
    );
  } finally {
    f.cleanup();
  }
});

test("buildToolsList tags capabilities in description", async () => {
  const f = await makeFixtureBinary();
  try {
    const binary = await discover(f.binPath);
    const tools = buildToolsList(binary);
    const write = tools.find((t) => t.name === "fixture-bin.write");
    assert.ok(write, "fixture-bin.write should be exposed");
    assert.match(write!.description, /side_effects: fs_write/);
  } finally {
    f.cleanup();
  }
});

test("buildToolsList passes through input_schema verbatim", async () => {
  const f = await makeFixtureBinary();
  try {
    const binary = await discover(f.binPath);
    const tools = buildToolsList(binary);
    const echo = tools.find((t) => t.name === "fixture-bin.echo");
    assert.ok(echo);
    const schema = echo!.inputSchema as { required: string[]; properties: Record<string, unknown> };
    assert.deepEqual(schema.required, ["message"]);
    assert.ok(schema.properties["tags"]);
  } finally {
    f.cleanup();
  }
});

// ── End-to-end SDK client → compiled CLI → fixture binary ─────────────

test("end-to-end: SDK client drives compiled CLI against a fixture binary", async () => {
  const f = await makeFixtureBinary();
  try {
    // Need the bridge to allow fs_write since the fixture has a write callable
    // (we'll only call echo, so default policy works). But let's allow all
    // so a subsequent test exercising write succeeds.
    const transport = new StdioClientTransport({
      command: process.execPath,
      args: [
        "/Users/clubpenguin/Documents/Korg/adapters/introspect-mcp-ts/dist/cli.js",
        f.binPath,
        "--allow",
        "all",
      ],
    });
    const client = new Client(
      { name: "smoke", version: "0.0.0" },
      { capabilities: {} }
    );
    await client.connect(transport);

    // tools/list should return all 4 fixture callables
    const toolsResp = await client.listTools();
    assert.equal(toolsResp.tools.length, 4);
    const names = new Set(toolsResp.tools.map((t) => t.name));
    assert.ok(names.has("fixture-bin.echo"));

    // tools/call for echo
    const echoResp = await client.callTool({
      name: "fixture-bin.echo",
      arguments: { message: "hello bridge" },
    });
    const content = echoResp.content as { type: string; text: string }[];
    assert.equal(content[0]!.type, "text");
    const text = content[0]!.text;
    const parsed = JSON.parse(text) as { argv: string[] };
    assert.ok(parsed.argv.includes("--message"));
    assert.ok(parsed.argv.includes("hello bridge"));

    // tools/call for session-mode shell should be refused with the
    // unsupported-message
    const shellResp = await client.callTool({
      name: "fixture-bin.shell",
      arguments: {},
    });
    const shellContent = shellResp.content as { type: string; text: string }[];
    assert.equal(shellResp.isError, true);
    assert.match(shellContent[0]!.text, /session/i);

    await client.close();
  } finally {
    f.cleanup();
  }
});

// ── Policy gating end-to-end ──────────────────────────────────────────

test("end-to-end: default policy refuses fs_write callable", async () => {
  const f = await makeFixtureBinary();
  try {
    const transport = new StdioClientTransport({
      command: process.execPath,
      args: [
        "/Users/clubpenguin/Documents/Korg/adapters/introspect-mcp-ts/dist/cli.js",
        f.binPath,
        // No --allow flag → default (none + fs_read only)
      ],
    });
    const client = new Client(
      { name: "smoke-restricted", version: "0.0.0" },
      { capabilities: {} }
    );
    await client.connect(transport);

    const writeResp = await client.callTool({
      name: "fixture-bin.write",
      arguments: { path: "/tmp/x" },
    });
    const content = writeResp.content as { type: string; text: string }[];
    assert.equal(writeResp.isError, true);
    assert.match(content[0]!.text, /fs_write/);
    assert.match(content[0]!.text, /KORG_INTROSPECT_MCP_ALLOW/);

    await client.close();
  } finally {
    f.cleanup();
  }
});

test("Policy.fromEnv with explicit allow lets fs_write through (smoke)", async () => {
  // Pure unit: just confirm the env-var path is exercised
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "fs_write" });
  assert.ok(p.allows("fs_write"));
});
