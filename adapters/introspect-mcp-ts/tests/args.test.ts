import { test } from "node:test";
import assert from "node:assert/strict";
import { buildArgv, kebab, valueToArgv } from "../src/args.js";

// ── kebab ─────────────────────────────────────────────────────────────

test("kebab: underscore → hyphen", () => {
  assert.equal(kebab("top_n"), "top-n");
  assert.equal(kebab("file_path"), "file-path");
});

test("kebab: already-kebab is idempotent", () => {
  assert.equal(kebab("top-n"), "top-n");
  assert.equal(kebab("file"), "file");
});

// ── valueToArgv ───────────────────────────────────────────────────────

test("valueToArgv: string", () => {
  assert.deepEqual(valueToArgv("hello"), ["hello"]);
});

test("valueToArgv: number", () => {
  assert.deepEqual(valueToArgv(42), ["42"]);
  assert.deepEqual(valueToArgv(3.14), ["3.14"]);
});

test("valueToArgv: bool raises", () => {
  assert.throws(() => valueToArgv(true), TypeError);
});

test("valueToArgv: object falls back to JSON", () => {
  assert.deepEqual(valueToArgv({ k: "v" }), ['{"k":"v"}']);
});

test("valueToArgv: null returns empty array", () => {
  assert.deepEqual(valueToArgv(null), []);
});

// ── buildArgv: scalars ────────────────────────────────────────────────

test("simple string arg", () => {
  const argv = buildArgv("/bin/thump", "thump.echo", "thump", { message: "hi" });
  assert.deepEqual(argv, ["/bin/thump", "echo", "--message", "hi"]);
});

test("snake_case arg becomes kebab-case flag", () => {
  const argv = buildArgv("/bin/x", "x.foo", "x", { top_n: 5 });
  assert.ok(argv.includes("--top-n"));
  assert.ok(argv.includes("5"));
});

test("bool true emits flag only", () => {
  const argv = buildArgv("/bin/x", "x.foo", "x", { quiet: true });
  assert.ok(argv.includes("--quiet"));
  const idx = argv.indexOf("--quiet");
  // Should be the last element (no value after)
  assert.equal(idx, argv.length - 1);
});

test("bool false omits flag", () => {
  const argv = buildArgv("/bin/x", "x.foo", "x", { quiet: false, message: "hi" });
  assert.ok(!argv.includes("--quiet"));
  assert.ok(argv.includes("--message"));
});

test("array arg repeats flag", () => {
  const argv = buildArgv("/bin/x", "x.foo", "x", { tags: ["a", "b", "c"] });
  const count = argv.filter((v) => v === "--tags").length;
  assert.equal(count, 3);
  for (const v of ["a", "b", "c"]) assert.ok(argv.includes(v));
});

test("null value omitted", () => {
  const argv = buildArgv("/bin/x", "x.foo", "x", { optional: null, message: "hi" });
  assert.ok(!argv.includes("--optional"));
  assert.ok(argv.includes("--message"));
});

// ── buildArgv: subcommand paths ───────────────────────────────────────

test("naked binary (command_id == binary_name) has no subcommand", () => {
  const argv = buildArgv("/bin/thump", "thump", "thump", { flag: "x" });
  assert.deepEqual(argv, ["/bin/thump", "--flag", "x"]);
});

test("one-segment subcommand", () => {
  const argv = buildArgv("/bin/thump", "thump.generate", "thump", { name: "x" });
  assert.deepEqual(argv, ["/bin/thump", "generate", "--name", "x"]);
});

test("nested subcommand path", () => {
  const argv = buildArgv("/bin/thump", "thump.bun.script.run", "thump", { name: "build" });
  assert.deepEqual(argv.slice(0, 5), ["/bin/thump", "bun", "script", "run", "--name"]);
});

test("dashed name in segment is kept whole", () => {
  const argv = buildArgv("/bin/korgex", "korgex.install-extension", "korgex", {});
  assert.deepEqual(argv, ["/bin/korgex", "install-extension"]);
});

test("command_id without binary prefix becomes pure path", () => {
  const argv = buildArgv("/bin/x", "some.other.path", "x", {});
  assert.deepEqual(argv, ["/bin/x", "some", "other", "path"]);
});
