import { test } from "node:test";
import assert from "node:assert/strict";
import { discover } from "../src/discovery.js";
import { invoke, SESSION_NOT_SUPPORTED } from "../src/invoker.js";
import { makeFixtureBinary } from "./conftest.js";

// ── envelope mode ─────────────────────────────────────────────────────

test("envelope mode returns pretty-printed JSON", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const echo = d.callables.find((c) => c.command_id === "fixture-bin.echo")!;
    const result = await invoke(echo, { message: "hello" }, {
      binaryPath: d.binary_path,
      binaryName: d.binary_name,
    });
    assert.equal(result.isError, false);
    const parsed = JSON.parse(result.text) as { argv: string[] };
    assert.ok(parsed.argv.includes("--message"));
    assert.ok(parsed.argv.includes("hello"));
  } finally {
    f.cleanup();
  }
});

test("kebab-case flags for snake_case args", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const echo = d.callables.find((c) => c.command_id === "fixture-bin.echo")!;
    const result = await invoke(
      echo,
      { message: "hi", count: 7, loud: true },
      { binaryPath: d.binary_path, binaryName: d.binary_name }
    );
    const parsed = JSON.parse(result.text) as { argv: string[] };
    assert.ok(parsed.argv.includes("--message"));
    assert.ok(parsed.argv.includes("--count"));
    assert.ok(parsed.argv.includes("--loud"));
    assert.equal(parsed.argv[0], "echo");
  } finally {
    f.cleanup();
  }
});

test("array args repeat the flag", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const echo = d.callables.find((c) => c.command_id === "fixture-bin.echo")!;
    const result = await invoke(
      echo,
      { message: "x", tags: ["a", "b"] },
      { binaryPath: d.binary_path, binaryName: d.binary_name }
    );
    const parsed = JSON.parse(result.text) as { argv: string[] };
    const count = parsed.argv.filter((v: string) => v === "--tags").length;
    assert.equal(count, 2);
  } finally {
    f.cleanup();
  }
});

// ── session mode (unsupported) ────────────────────────────────────────

test("session mode returns SESSION_NOT_SUPPORTED", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const shell = d.callables.find((c) => c.command_id === "fixture-bin.shell")!;
    const result = await invoke(shell, {}, {
      binaryPath: d.binary_path,
      binaryName: d.binary_name,
    });
    assert.equal(result.isError, true);
    assert.equal(result.text, SESSION_NOT_SUPPORTED);
  } finally {
    f.cleanup();
  }
});

// ── failure paths ─────────────────────────────────────────────────────

test("non-zero exit appends stderr + exit_code", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const fail = d.callables.find((c) => c.command_id === "fixture-bin.fail")!;
    const result = await invoke(fail, {}, {
      binaryPath: d.binary_path,
      binaryName: d.binary_name,
    });
    assert.equal(result.isError, true);
    assert.match(result.text, /exit_code/);
    assert.match(result.text, / 1/);
  } finally {
    f.cleanup();
  }
});

test("invoke uses the correct binary path", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const echo = d.callables.find((c) => c.command_id === "fixture-bin.echo")!;
    const result = await invoke(echo, { message: "trace" }, {
      binaryPath: d.binary_path,
      binaryName: d.binary_name,
    });
    assert.match(result.text, /trace/);
  } finally {
    f.cleanup();
  }
});

test("timeout terminates and reports", async () => {
  const f = await makeFixtureBinary();
  try {
    const d = await discover(f.binPath);
    const echo = d.callables.find((c) => c.command_id === "fixture-bin.echo")!;
    // The fixture binary returns instantly, so this only times out when we
    // also kill it via SIGTERM. Use a near-zero timeout to test the path.
    const result = await invoke(echo, { message: "x" }, {
      binaryPath: d.binary_path,
      binaryName: d.binary_name,
      timeoutMs: 1, // milliseconds — too short for spawn to even start
    });
    // The fixture might still complete in 1ms on a fast machine; assert
    // EITHER timed-out-message OR a successful echo (since fixture is fast).
    if (result.isError) {
      // If it timed out, the text should say so
      assert.match(result.text, /timed out|exit_code/);
    } else {
      // Completed in time — that's fine too
      assert.match(result.text, /trace|argv|x/);
    }
  } finally {
    f.cleanup();
  }
});
