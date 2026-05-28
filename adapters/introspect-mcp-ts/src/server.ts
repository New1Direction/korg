// MCP server: registers one tool per discovered callable. Tool name is
// the command_id directly so cross-tool agents (recall → re-invoke)
// see the same identifier.

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import type { DiscoveredBinary } from "./discovery.js";
import { findCallableByCommandId } from "./discovery.js";
import { invoke } from "./invoker.js";
import type { Policy } from "./safety.js";

export const SERVER_NAME = "korg-introspect-mcp";
export const SERVER_VERSION = "0.1.0";

export function buildToolsList(binary: DiscoveredBinary): {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}[] {
  const tools = [];
  for (const c of binary.callables) {
    const cap = c.capabilities as Record<string, unknown>;
    const tags =
      `[side_effects: ${String(cap["side_effects"] ?? "unknown")}, ` +
      `output_mode: ${String(cap["output_mode"] ?? "unknown")}, ` +
      `long_running: ${String(cap["long_running"] ?? false)}]`;
    tools.push({
      name: c.command_id,
      description: ((c.description || c.name) + " " + tags).trim(),
      inputSchema: c.input_schema,
    });
  }
  return tools;
}

export function buildServer(binary: DiscoveredBinary, policy: Policy): Server {
  const server = new Server(
    { name: `${SERVER_NAME}(${binary.binary_name})`, version: SERVER_VERSION },
    { capabilities: { tools: {} } }
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => {
    return { tools: buildToolsList(binary) };
  });

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const name = String(request.params.name);
    const args = (request.params.arguments ?? {}) as Record<string, unknown>;
    const callableDef = findCallableByCommandId(binary, name);
    if (!callableDef) {
      throw new Error(`unknown tool: ${name}`);
    }
    const sideEffects = String(
      (callableDef.capabilities as { side_effects?: string }).side_effects ?? "none"
    );
    if (!policy.allows(sideEffects)) {
      return {
        content: [{ type: "text", text: policy.explainDenial(sideEffects) }],
        isError: true,
      };
    }
    try {
      const result = await invoke(callableDef, args, {
        binaryPath: binary.binary_path,
        binaryName: binary.binary_name,
      });
      return {
        content: [{ type: "text", text: result.text }],
        isError: result.isError,
      };
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return {
        content: [{ type: "text", text: `[korg-introspect-mcp] invocation failed: ${msg}` }],
        isError: true,
      };
    }
  });

  return server;
}

export async function serveStdio(binary: DiscoveredBinary, policy: Policy): Promise<void> {
  const server = buildServer(binary, policy);
  const transport = new StdioServerTransport();
  // Wire close handler before connecting (subprocesses can EOF very fast in tests).
  const closed = new Promise<void>((resolve) => {
    transport.onclose = () => resolve();
  });
  await server.connect(transport);
  await closed;
}
