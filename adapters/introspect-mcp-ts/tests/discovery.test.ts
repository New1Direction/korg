import { test } from "node:test";
import assert from "node:assert/strict";
import { DiscoveryError, discover, resolveBinary, validateDocument, runIntrospect } from "../src/discovery.js";
import { makeFixtureBinary } from "./conftest.js";

// ── resolveBinary ─────────────────────────────────────────────────────

test("resolveBinary: absolute path that exists", async () => {
  const f = await makeFixtureBinary();
  try {
    const p = await resolveBinary(f.binPath);
    assert.equal(p, f.binPath);
  } finally {
    f.cleanup();
  }
});

test("resolveBinary: missing absolute path raises", async () => {
  await assert.rejects(
    () => resolveBinary("/tmp/definitely-does-not-exist-9876"),
    DiscoveryError
  );
});

test("resolveBinary: missing PATH name raises", async () => {
  await assert.rejects(
    () => resolveBinary("definitely-not-a-real-binary-9876"),
    DiscoveryError
  );
});

// ── runIntrospect ─────────────────────────────────────────────────────

test("runIntrospect returns parsed dict", async () => {
  const f = await makeFixtureBinary();
  try {
    const doc = (await runIntrospect(f.binPath)) as { schema: string; binary: string };
    assert.equal(doc.schema, "korg:introspect@v1");
    assert.equal(doc.binary, "fixture-bin");
  } finally {
    f.cleanup();
  }
});

test("runIntrospect: non-JSON output raises", async () => {
  const f = await makeFixtureBinary({ stdoutOverride: "not json at all" });
  try {
    await assert.rejects(() => runIntrospect(f.binPath), DiscoveryError);
  } finally {
    f.cleanup();
  }
});

// ── validateDocument ──────────────────────────────────────────────────

test("validate rejects wrong schema", async () => {
  assert.throws(
    () => validateDocument({ schema: "some:other@v1", callables: [] }, "/x"),
    DiscoveryError
  );
});

test("validate rejects missing callables array", async () => {
  assert.throws(() => validateDocument({ schema: "korg:introspect@v1" }, "/x"), DiscoveryError);
});

test("validate rejects duplicate command_ids", async () => {
  const doc = {
    schema: "korg:introspect@v1",
    callables: [
      { command_id: "x.a", name: "a", input_schema: {}, capabilities: {} },
      { command_id: "x.a", name: "a", input_schema: {}, capabilities: {} },
    ],
  };
  assert.throws(() => validateDocument(doc, "/x"), DiscoveryError);
});

test("validate rejects missing required fields", async () => {
  const doc = {
    schema: "korg:introspect@v1",
    callables: [{ command_id: "x.a", name: "a" }],
  };
  assert.throws(() => validateDocument(doc, "/x"), DiscoveryError);
});

test("validate accepts minimal valid doc", () => {
  const result = validateDocument(
    {
      schema: "korg:introspect@v1",
      binary: "x",
      version: "1.2.3",
      callables: [
        {
          command_id: "x.a",
          name: "a",
          description: "",
          input_schema: { type: "object" },
          capabilities: { side_effects: "none" },
        },
      ],
    },
    "/x"
  );
  assert.equal(result.binary_name, "x");
  assert.equal(result.version, "1.2.3");
  assert.equal(result.callables.length, 1);
});

// ── discover end-to-end ───────────────────────────────────────────────

test("discover end-to-end with fixture binary", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    assert.equal(d.binary_name, "fixture-bin");
    assert.equal(d.callables_declared, true);
    assert.equal(d.callables.length, 4);
    const ids = new Set(d.callables.map((c) => c.command_id));
    assert.ok(ids.has("fixture-bin.echo"));
    assert.ok(ids.has("fixture-bin.write"));
    assert.ok(ids.has("fixture-bin.shell"));
    assert.ok(ids.has("fixture-bin.fail"));
  } finally {
    f.cleanup();
  }
});
