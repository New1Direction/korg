// MCP server wiring — uses the low-level Server class from the official
// SDK so we have direct control over the wire format. tools/list and
// tools/call are mapped to RecallEngine.search() outputs.

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { RecallEngine, type Match, type Mode } from "./search.js";
import {
  callableToMcpTool,
  getCallables,
} from "./introspect.js";

export const SERVER_NAME = "korg-recall-mcp";
export const SERVER_VERSION = "0.1.0";

export function formatMatchesForLlm(matches: readonly Match[], mode: string): string {
  if (matches.length === 0) {
    return `[recall · ${mode}] no relevant matches.`;
  }
  const lines = [`[recall · ${mode}] ${matches.length} match(es):`];
  for (const m of matches) {
    const ev = m.event;
    const agent = ev.sourceAgent.replace("agent:", "").slice(0, 40);
    const snippet = ev.embedText.replace(/\n/g, " ").slice(0, 200);
    lines.push(
      `  · seq=${ev.seq} score=${m.score.toFixed(2)} agent=${agent} ` +
        `tool=${ev.toolName} :: ${snippet}`
    );
  }
  return lines.join("\n");
}

export interface RecallArguments {
  query?: unknown;
  top_n?: unknown;
  min_score?: unknown;
  mode?: unknown;
  tool_filter?: unknown;
}

export async function handleRecallCall(
  engine: RecallEngine,
  args: RecallArguments
): Promise<{ content: { type: "text"; text: string }[]; isError?: boolean }> {
  const query = String(args.query ?? "").trim();
  if (!query) {
    return {
      content: [{ type: "text", text: "[recall] empty query." }],
      isError: true,
    };
  }
  const topN = typeof args.top_n === "number" ? args.top_n : undefined;
  const minScore = typeof args.min_score === "number" ? args.min_score : undefined;
  const modeRaw = typeof args.mode === "string" ? args.mode : "auto";
  const mode: Mode =
    modeRaw === "semantic" || modeRaw === "substring" || modeRaw === "auto"
      ? modeRaw
      : "auto";
  const toolFilter = Array.isArray(args.tool_filter)
    ? (args.tool_filter as unknown[]).map((v) => String(v))
    : undefined;

  try {
    const matches = await engine.search(query, {
      mode,
      topN,
      minScore,
      toolFilter,
    });
    const usedMode = engine.lastMode ?? mode;
    return {
      content: [{ type: "text", text: formatMatchesForLlm(matches, usedMode) }],
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return {
      content: [{ type: "text", text: `[recall] error: ${msg}` }],
      isError: true,
    };
  }
}

export function buildServer(engine: RecallEngine): Server {
  const server = new Server(
    { name: SERVER_NAME, version: SERVER_VERSION },
    { capabilities: { tools: {} } }
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => {
    return {
      tools: getCallables().map(callableToMcpTool),
    };
  });

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    if (name !== "recall") {
      throw new Error(`unknown tool: ${name}`);
    }
    return await handleRecallCall(engine, (args ?? {}) as RecallArguments);
  });

  return server;
}

export async function serveStdio(engine: RecallEngine): Promise<void> {
  const server = buildServer(engine);
  const transport = new StdioServerTransport();
  // Wire the close handler BEFORE connecting so we don't miss an
  // already-closed transport (subprocesses can EOF very fast in tests).
  const closed = new Promise<void>((resolve) => {
    transport.onclose = () => resolve();
  });
  await server.connect(transport);
  // server.connect() resolves once the transport is wired up — it does
  // NOT block for the lifetime of the connection. Hold open here until
  // the transport closes (stdin EOF / client disconnect) so the parent
  // process keeps reading messages from us.
  await closed;
}
