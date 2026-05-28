import { test } from "node:test";
import assert from "node:assert/strict";
import { execSync } from "node:child_process";
import { makeFixtureBinary } from "./conftest.js";

test("fixture binary spawns and responds to --introspect", async () => {
  const f = await makeFixtureBinary();
  try {
    const out = execSync(`${f.binPath} --introspect`, { encoding: "utf8" });
    const doc = JSON.parse(out);
    assert.equal(doc.schema, "korg:introspect@v1");
    assert.equal(doc.binary, "fixture-bin");
    assert.equal(doc.callables.length, 4);
  } finally {
    f.cleanup();
  }
});
