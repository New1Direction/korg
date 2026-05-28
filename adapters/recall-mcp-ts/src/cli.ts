#!/usr/bin/env node
// CLI entry. Default behavior: run the MCP server on stdio.
// --introspect: emit the korg:introspect@v1 document and exit.
// --query: one-shot search instead of running the server (for testing).

import { homedir } from "node:os";
import { resolve as resolvePath } from "node:path";
import { EventIndex } from "./event-index.js";
import { RecallEngine } from "./search.js";
import { buildIntrospectDocument } from "./introspect.js";
import { formatMatchesForLlm, serveStdio, SERVER_VERSION } from "./server.js";

interface ParsedArgs {
  ledger: string[];
  query: string | null;
  topN: number;
  minScore: number;
  mode: "auto" | "semantic" | "substring";
  introspect: boolean;
  help: boolean;
}

function expanduser(p: string): string {
  if (p.startsWith("~")) return resolvePath(homedir() + p.slice(1));
  return resolvePath(p);
}

function parseArgs(argv: readonly string[]): ParsedArgs {
  const out: ParsedArgs = {
    ledger: [],
    query: null,
    topN: 5,
    minScore: 0.30,
    mode: "auto",
    introspect: false,
    help: false,
  };

  // Transparent-accept a leading "recall" token so invocations via the
  // introspect-mcp bridge work (mirrors the Python version's fix).
  const args = [...argv];
  if (args[0] === "recall") args.shift();

  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--help" || a === "-h") out.help = true;
    else if (a === "--introspect") out.introspect = true;
    else if (a === "--ledger") {
      const v = args[++i];
      if (v) out.ledger.push(v);
    } else if (a === "--query") {
      const v = args[++i];
      if (v) out.query = v;
    } else if (a === "--top-n") {
      const v = args[++i];
      if (v) out.topN = parseInt(v, 10);
    } else if (a === "--min-score") {
      const v = args[++i];
      if (v) out.minScore = parseFloat(v);
    } else if (a === "--mode") {
      const v = args[++i];
      if (v === "auto" || v === "semantic" || v === "substring") out.mode = v;
    }
  }

  return out;
}

const HELP = `korg-recall-mcp — cross-session semantic recall over the korg ledger.

Usage:
  korg-recall-mcp                              run as an MCP server on stdio (default)
  korg-recall-mcp --query "rust borrow"        one-shot CLI search and exit
  korg-recall-mcp --introspect                 emit korg:introspect@v1 document

Options:
  --ledger PATH       Path to a ledger .jsonl file or directory (repeatable).
                      Default: ~/.korg/claude-events.jsonl
  --query Q           One-shot CLI search.
  --top-n N           Max number of matches (default 5).
  --min-score F       Cosine-similarity floor for semantic mode (default 0.30).
  --mode auto|semantic|substring   Recall strategy (default auto).
  --help, -h          Show this help.

For continuous capture, pair with the Python korg-ingest-claude --tail
running in another process (or use korg-setup to install both).
`;

export async function main(argv: readonly string[]): Promise<number> {
  const args = parseArgs(argv);
  if (args.help) {
    console.error(HELP);
    return 0;
  }
  if (args.introspect) {
    console.log(JSON.stringify(buildIntrospectDocument(SERVER_VERSION), null, 2));
    return 0;
  }

  const ledgerPaths =
    args.ledger.length > 0
      ? args.ledger.map(expanduser)
      : [expanduser("~/.korg/claude-events.jsonl")];

  const index = new EventIndex(ledgerPaths);
  await index.refresh();
  const engine = new RecallEngine(index);

  if (args.query !== null) {
    const matches = await engine.search(args.query, {
      mode: args.mode,
      topN: args.topN,
      minScore: args.minScore,
    });
    const usedMode = engine.lastMode ?? args.mode;
    console.log(formatMatchesForLlm(matches, usedMode));
    return 0;
  }

  console.error(
    `[korg-recall-mcp] starting MCP server on stdio. ` +
      `ledger paths: ${JSON.stringify(ledgerPaths)}. ` +
      `loaded ${index.length} event(s) at startup.`
  );
  await serveStdio(engine);
  return 0;
}

main(process.argv.slice(2)).then(
  (rc) => process.exit(rc),
  (err) => {
    console.error("[korg-recall-mcp] fatal:", err);
    process.exit(1);
  }
);
