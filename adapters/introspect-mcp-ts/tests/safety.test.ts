import { test } from "node:test";
import assert from "node:assert/strict";
import { ALL_EFFECTS, ALWAYS_ALLOWED, Policy } from "../src/safety.js";

// ── Defaults ──────────────────────────────────────────────────────────

test("default policy allows none + fs_read", () => {
  const p = Policy.fromEnv({});
  assert.ok(p.allows("none"));
  assert.ok(p.allows("fs_read"));
});

test("default policy denies writes + network", () => {
  const p = Policy.fromEnv({});
  assert.ok(!p.allows("fs_write"));
  assert.ok(!p.allows("network"));
  assert.ok(!p.allows("ledger_write"));
});

test("readOnly factory == defaults", () => {
  assert.deepEqual([...Policy.readOnly().allowed].sort(), [...ALWAYS_ALLOWED].sort());
});

test("all factory allows everything", () => {
  const p = Policy.all();
  for (const e of ALL_EFFECTS) {
    assert.ok(p.allows(e), `should allow ${e}`);
  }
});

// ── Env var parsing ───────────────────────────────────────────────────

test("env var single value", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "fs_write" });
  assert.ok(p.allows("fs_write"));
  assert.ok(p.allows("fs_read")); // defaults still hold
  assert.ok(!p.allows("network"));
});

test("env var comma-separated", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "fs_write,network,ledger_write" });
  assert.ok(p.allows("fs_write"));
  assert.ok(p.allows("network"));
  assert.ok(p.allows("ledger_write"));
});

test("env var whitespace tolerant", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "  fs_write , network  " });
  assert.ok(p.allows("fs_write"));
  assert.ok(p.allows("network"));
});

test("env var 'all' keyword", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "all" });
  assert.deepEqual([...p.allowed].sort(), [...ALL_EFFECTS].sort());
});

test("env var '*' keyword", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "*" });
  assert.deepEqual([...p.allowed].sort(), [...ALL_EFFECTS].sort());
});

test("env var case-insensitive", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "ALL" });
  assert.deepEqual([...p.allowed].sort(), [...ALL_EFFECTS].sort());
});

test("empty env var keeps defaults", () => {
  const p = Policy.fromEnv({ KORG_INTROSPECT_MCP_ALLOW: "" });
  assert.deepEqual([...p.allowed].sort(), [...ALWAYS_ALLOWED].sort());
});

test("missing env var keeps defaults", () => {
  const p = Policy.fromEnv({});
  assert.deepEqual([...p.allowed].sort(), [...ALWAYS_ALLOWED].sort());
});

// ── explainDenial ─────────────────────────────────────────────────────

test("explainDenial mentions effect + env var", () => {
  const p = Policy.readOnly();
  const msg = p.explainDenial("fs_write");
  assert.match(msg, /fs_write/);
  assert.match(msg, /KORG_INTROSPECT_MCP_ALLOW/);
});
