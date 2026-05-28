import { test } from "node:test";
import assert from "node:assert/strict";
import { textForEvent } from "../src/text.js";

test("user_prompt extracts prompt arg", () => {
  assert.equal(
    textForEvent({
      tool_name: "user_prompt",
      args: { prompt: "what is the rust borrow checker" },
      result: {},
    }),
    "what is the rust borrow checker"
  );
});

test("llm_inference extracts result.text", () => {
  assert.equal(
    textForEvent({
      tool_name: "llm_inference",
      args: { model: "claude-opus-4-7" },
      result: { text: "Rust enforces ownership at compile time." },
    }),
    "Rust enforces ownership at compile time."
  );
});

test("llm_inference with no text is empty", () => {
  assert.equal(
    textForEvent({
      tool_name: "llm_inference",
      args: {},
      result: { completion_tokens: 2 },
    }),
    ""
  );
});

test("tool call includes name and key args", () => {
  const out = textForEvent({
    tool_name: "Read",
    args: { file_path: "/src/main.rs", offset: 0 },
    result: { output: 'fn main() { println!("hello"); }' },
  });
  assert.ok(out.includes("Read"));
  assert.ok(out.includes("/src/main.rs"));
  assert.ok(out.includes("println"));
});

test("Bash command is findable by command + result", () => {
  const out = textForEvent({
    tool_name: "Bash",
    args: { command: "pytest tests/test_auth.py", description: "run auth tests" },
    result: { output: "5 passed in 0.42s" },
  });
  assert.ok(out.includes("Bash"));
  assert.ok(out.includes("pytest"));
  assert.ok(out.includes("5 passed"));
});

test("unknown args fall back to json dump", () => {
  const out = textForEvent({
    tool_name: "MysteryTool",
    args: { weird_field: "important value here" },
    result: {},
  });
  assert.ok(out.includes("MysteryTool"));
  assert.ok(out.includes("important value here"));
});

test("long text is trimmed", () => {
  const out = textForEvent({
    tool_name: "user_prompt",
    args: { prompt: "x".repeat(5000) },
    result: {},
  });
  assert.ok(out.length < 5000);
  assert.ok(out.endsWith("…"));
});

test("empty event returns empty string", () => {
  assert.equal(textForEvent({}), "");
});

test("missing args and result keys", () => {
  assert.equal(textForEvent({ tool_name: "user_prompt" }), "");
});
