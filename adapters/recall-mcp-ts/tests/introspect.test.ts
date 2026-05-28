import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildIntrospectDocument,
  callableToIntrospectEntry,
  callableToMcpTool,
  EXIT_CODES,
  getCallables,
  INTROSPECT_SCHEMA_ID,
} from "../src/introspect.js";

// ── Schema invariants ─────────────────────────────────────────────────

test("uses canonical korg:introspect@v1 schema id", () => {
  assert.equal(INTROSPECT_SCHEMA_ID, "korg:introspect@v1");
});

test("document carries schema, binary, version", () => {
  const doc = buildIntrospectDocument("9.9.9") as {
    schema: string; binary: string; version: string; callables_declared: boolean;
  };
  assert.equal(doc.schema, "korg:introspect@v1");
  assert.equal(doc.binary, "korg-recall-mcp");
  assert.equal(doc.version, "9.9.9");
  assert.equal(doc.callables_declared, true);
});

test("callables are unique by command_id", () => {
  const ids = getCallables().map((c) => c.command_id);
  assert.equal(new Set(ids).size, ids.length);
});

test("command_id is namespaced with korg-recall-mcp prefix", () => {
  for (const c of getCallables()) {
    assert.ok(c.command_id.startsWith("korg-recall-mcp"), `bad id: ${c.command_id}`);
    assert.ok(!c.command_id.includes(" "));
  }
});

test("recognized side_effects values only", () => {
  const valid = new Set(["none", "fs_read", "fs_write", "network", "ledger_write"]);
  for (const c of getCallables()) {
    assert.ok(valid.has(c.capabilities.side_effects), `bad: ${c.command_id} -> ${c.capabilities.side_effects}`);
  }
});

test("recognized output_mode values only", () => {
  const valid = new Set(["none", "stream", "envelope", "session"]);
  for (const c of getCallables()) {
    assert.ok(valid.has(c.capabilities.output_mode), `bad: ${c.command_id} -> ${c.capabilities.output_mode}`);
  }
});

test("input_schema is object-typed", () => {
  for (const c of getCallables()) {
    const sch = c.input_schema as { type?: string };
    assert.equal(sch.type, "object", `bad schema for ${c.command_id}`);
  }
});

// ── MCP vs introspect drift check ─────────────────────────────────────

test("MCP tool projection and introspect entry use the SAME input_schema object", () => {
  const [recall] = getCallables();
  assert.ok(recall);
  const mcpTool = callableToMcpTool(recall!);
  const introspectEntry = callableToIntrospectEntry(recall!) as { input_schema: unknown };

  // MCP uses inputSchema (camel), introspect uses input_schema (snake) —
  // but both reference the same underlying object, so the field shape
  // can't drift between the two surfaces.
  assert.deepEqual(mcpTool.inputSchema, introspectEntry.input_schema);
});

test("Python reference parity: command_id matches", () => {
  // The Python adapters/recall-mcp registers command_id "korg-recall-mcp.recall".
  // The TS port must match exactly so cross-tool agents see one identifier.
  const ids = getCallables().map((c) => c.command_id);
  assert.ok(ids.includes("korg-recall-mcp.recall"));
});

// ── Exit codes table ──────────────────────────────────────────────────

test("exit codes table has canonical entries with string keys", () => {
  assert.equal(EXIT_CODES["0"], "success");
  assert.equal(EXIT_CODES["1"], "error.generic");
  assert.equal(EXIT_CODES["2"], "error.usage");
  for (const k of Object.keys(EXIT_CODES)) {
    assert.match(k, /^\d+$/);
  }
});

test("document round-trips through JSON", () => {
  const doc = buildIntrospectDocument("0.1.0");
  const blob = JSON.stringify(doc);
  const parsed = JSON.parse(blob);
  assert.deepEqual(parsed, doc);
});
