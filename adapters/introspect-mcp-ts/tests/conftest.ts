// Test helper: build a synthetic --introspect-aware binary in a tmp dir
// that responds to --introspect with a known doc and echoes argv otherwise.

import { promises as fs } from "node:fs";
import { mkdtempSync, chmodSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

export interface FixtureOptions {
  name?: string;
  schema?: string;
  callables?: object[] | null; // null → omit (for testing malformed docs)
  exitCode?: number;
  stdoutOverride?: string;
}

const DEFAULT_CALLABLES = (name: string) => [
  {
    command_id: `${name}.echo`,
    name: "echo",
    description: "Echo args back as JSON.",
    surfaces: ["cli"],
    input_schema: {
      type: "object",
      properties: {
        message: { type: "string" },
        count: { type: "integer" },
        loud: { type: "boolean" },
        tags: { type: "array", items: { type: "string" } },
      },
      required: ["message"],
    },
    capabilities: {
      output_mode: "envelope",
      side_effects: "none",
      requires_project: false,
      long_running: false,
      stateful: false,
      reads_stdin: false,
      supports_output_path: false,
    },
  },
  {
    command_id: `${name}.write`,
    name: "write",
    description: "Pretend to write a file.",
    surfaces: ["cli"],
    input_schema: {
      type: "object",
      properties: { path: { type: "string" } },
      required: ["path"],
    },
    capabilities: {
      output_mode: "envelope",
      side_effects: "fs_write",
      requires_project: false,
      long_running: false,
      stateful: false,
      reads_stdin: false,
      supports_output_path: true,
    },
  },
  {
    command_id: `${name}.shell`,
    name: "shell",
    description: "Open a stateful session.",
    surfaces: ["cli"],
    input_schema: { type: "object" },
    capabilities: {
      output_mode: "session",
      side_effects: "ledger_write",
      requires_project: false,
      long_running: true,
      stateful: true,
      reads_stdin: true,
      supports_output_path: false,
    },
  },
  {
    command_id: `${name}.fail`,
    name: "fail",
    description: "Always exits 1.",
    surfaces: ["cli"],
    input_schema: { type: "object" },
    capabilities: {
      output_mode: "envelope",
      side_effects: "none",
      requires_project: false,
      long_running: false,
      stateful: false,
      reads_stdin: false,
      supports_output_path: false,
    },
  },
];

export interface Fixture {
  binPath: string;
  dir: string;
  cleanup: () => void;
}

export async function makeFixtureBinary(opts: FixtureOptions = {}): Promise<Fixture> {
  const name = opts.name ?? "fixture-bin";
  const dir = mkdtempSync(join(tmpdir(), "im-fixture-"));

  let docContent: string;
  if (opts.stdoutOverride !== undefined) {
    docContent = opts.stdoutOverride;
  } else {
    const doc: Record<string, unknown> = {
      schema: opts.schema ?? "korg:introspect@v1",
      binary: name,
      version: "0.0.1",
      callables_declared: true,
      exit_codes: { "0": "success", "1": "error.generic", "2": "error.usage" },
    };
    if (opts.callables !== null) {
      doc["callables"] = opts.callables ?? DEFAULT_CALLABLES(name);
    }
    docContent = JSON.stringify(doc);
  }
  const docPath = join(dir, `${name}.introspect.json`);
  await fs.writeFile(docPath, docContent);

  const exitCode = opts.exitCode ?? 0;
  // Plain Node script using the absolute path to the running Node interpreter
  // in the shebang. This works reliably on macOS and Linux. We escape the
  // backslashes in docPath in case the test runs on Windows (where backslashes
  // would otherwise be interpreted as JS string escapes).
  const escapedDocPath = JSON.stringify(docPath);
  const body = `#!${process.execPath}
const fs = require('node:fs');
const args = process.argv.slice(2);
if (args.includes("--introspect")) {
  process.stdout.write(fs.readFileSync(${escapedDocPath}, "utf8"));
  process.exit(${exitCode});
}
if (args[0] === "fail") {
  console.log(JSON.stringify({ failed: true }));
  process.exit(1);
}
console.log(JSON.stringify({ argv: args }));
process.exit(0);
`;
  const binPath = join(dir, name);
  await fs.writeFile(binPath, body);
  chmodSync(binPath, 0o755);

  return {
    binPath,
    dir,
    cleanup: () => {
      try {
        rmSync(dir, { recursive: true, force: true });
      } catch {
        /* ignore */
      }
    },
  };
}
