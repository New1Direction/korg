#!/usr/bin/env node

import { discover, DiscoveryError } from "./discovery.js";
import { ALL_EFFECTS, Policy } from "./safety.js";
import { serveStdio, SERVER_VERSION } from "./server.js";

interface ParsedArgs {
  binary: string | null;
  allow: string | null;
  listTools: boolean;
  help: boolean;
  version: boolean;
}

function parseArgs(argv: readonly string[]): ParsedArgs {
  const out: ParsedArgs = {
    binary: null,
    allow: null,
    listTools: false,
    help: false,
    version: false,
  };
  let positional: string | null = null;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") out.help = true;
    else if (a === "--version") out.version = true;
    else if (a === "--list-tools") out.listTools = true;
    else if (a === "--allow") {
      const v = argv[++i];
      if (v) out.allow = v;
    } else if (a && !a.startsWith("-") && positional === null) {
      positional = a;
    }
  }
  out.binary = positional;
  return out;
}

const HELP = `korg-introspect-mcp — bridge any --introspect-aware binary to MCP.

Usage:
  korg-introspect-mcp <binary> [--allow SIDE_EFFECTS] [--list-tools]

Arguments:
  binary             Path or name of the binary to wrap. Must support --introspect
                     and emit a korg:introspect@v1 document.

Options:
  --allow ALLOW      Comma-separated side_effects to allow (in addition to the
                     always-allowed 'none' and 'fs_read'). Recognized:
                     ${[...ALL_EFFECTS].sort().join(", ")}.
                     Use 'all' to allow everything. Overrides
                     KORG_INTROSPECT_MCP_ALLOW.
  --list-tools       Print the discovered tools to stderr and exit.
  --version          Print version and exit.
  --help, -h         Print this help.

Example:
  korg-introspect-mcp thump --allow fs_write
  korg-introspect-mcp /usr/local/bin/korgex --allow all
  korg-introspect-mcp thump --list-tools
`;

export async function main(argv: readonly string[]): Promise<number> {
  const args = parseArgs(argv);
  if (args.help) {
    console.error(HELP);
    return 0;
  }
  if (args.version) {
    console.log(SERVER_VERSION);
    return 0;
  }
  if (!args.binary) {
    console.error(HELP);
    console.error("\n[korg-introspect-mcp] error: missing required argument: binary");
    return 2;
  }

  let binary;
  try {
    binary = await discover(args.binary);
  } catch (e) {
    const msg = e instanceof DiscoveryError ? e.message : (e as Error).message;
    console.error(`[korg-introspect-mcp] ${msg}`);
    return 1;
  }

  if (args.allow !== null) {
    process.env["KORG_INTROSPECT_MCP_ALLOW"] = args.allow;
  }
  const policy = Policy.fromEnv();

  if (args.listTools) {
    console.error(
      `[korg-introspect-mcp] ${binary.binary_name} v${binary.version} — ` +
        `${binary.callables.length} tool(s):`
    );
    for (const c of binary.callables) {
      const cap = c.capabilities as Record<string, unknown>;
      const se = String(cap["side_effects"] ?? "?");
      const om = String(cap["output_mode"] ?? "?");
      const lr = String(cap["long_running"] ?? false);
      const denied = !policy.allows(se) ? " (would be denied)" : "";
      console.error(
        `  ${c.command_id.padEnd(36)} side_effects=${se.padEnd(14)} ` +
          `output_mode=${om.padEnd(10)} long_running=${lr.padEnd(5)}${denied}`
      );
    }
    console.error(
      `[korg-introspect-mcp] policy: allowed side_effects = ${JSON.stringify([...policy.allowed].sort())}`
    );
    return 0;
  }

  console.error(
    `[korg-introspect-mcp] serving ${binary.binary_name} v${binary.version} — ` +
      `${binary.callables.length} callables, ` +
      `policy=${JSON.stringify([...policy.allowed].sort())}`
  );
  await serveStdio(binary, policy);
  return 0;
}

main(process.argv.slice(2)).then(
  (rc) => process.exit(rc),
  (err) => {
    console.error("[korg-introspect-mcp] fatal:", err);
    process.exit(1);
  }
);
