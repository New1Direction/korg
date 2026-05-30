// The korg:introspect@v1 document for @korgg/recall-mcp.
// MUST stay schema-aligned with the Python reference at
// adapters/recall-mcp/src/korg_recall_mcp/introspect.py so cross-binary
// agents see one schema across the ecosystem.

export const INTROSPECT_SCHEMA_ID = "korg:introspect@v1";
export const BINARY_NAME = "korg-recall-mcp";

export interface Capabilities {
  output_mode: "none" | "stream" | "envelope" | "session";
  side_effects: "none" | "fs_read" | "fs_write" | "network" | "ledger_write";
  requires_project: boolean;
  long_running: boolean;
  stateful: boolean;
  reads_stdin: boolean;
  supports_output_path: boolean;
}

const safeCapabilities = (override: Partial<Capabilities> = {}): Capabilities => ({
  output_mode: "envelope",
  side_effects: "none",
  requires_project: false,
  long_running: false,
  stateful: false,
  reads_stdin: false,
  supports_output_path: false,
  ...override,
});

export interface Callable {
  command_id: string;
  name: string;
  description: string;
  surfaces: readonly string[];
  input_schema: Record<string, unknown>;
  capabilities: Capabilities;
}

export const EXIT_CODES: Record<string, string> = {
  "0": "success",
  "1": "error.generic",
  "2": "error.usage",
  "3": "error.config",
  "4": "error.io",
  "5": "error.network",
  "6": "error.user_interrupt",
  "7": "error.dependency_missing",
};

export function getCallables(): Callable[] {
  return [
    {
      command_id: "korg-recall-mcp.recall",
      name: "recall",
      description:
        "Search across all prior AI sessions recorded in the korg ledger. " +
        "Returns relevant past prompts, model replies, and tool calls/results. " +
        "Use this BEFORE attempting work that may have been done before — " +
        "finding the prior session saves the cost of rediscovery.",
      surfaces: ["cli", "mcp"],
      input_schema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            description: "Natural-language description of what you're looking for.",
          },
          top_n: {
            type: "integer",
            description: "Max number of results (default 5).",
            default: 5,
            minimum: 1,
            maximum: 50,
          },
          min_score: {
            type: "number",
            description:
              "Cosine-similarity floor for semantic matches (default 0.30). " +
              "Ignored for substring mode.",
            default: 0.30,
            minimum: 0.0,
            maximum: 1.0,
          },
          mode: {
            type: "string",
            enum: ["auto", "semantic", "substring"],
            description:
              "auto: semantic if @xenova/transformers installed else substring. " +
              "semantic: require embedding-backed ranking. " +
              "substring: pure keyword AND-of-terms.",
            default: "auto",
          },
          tool_filter: {
            type: "array",
            items: { type: "string" },
            description:
              "Optional list of tool_name values to restrict the search to " +
              "(e.g. ['user_prompt'], ['Read', 'Bash']).",
          },
        },
        required: ["query"],
      },
      capabilities: safeCapabilities({
        output_mode: "stream",
        side_effects: "fs_read",
      }),
    },
  ];
}

export function callableToMcpTool(c: Callable): {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
} {
  return {
    name: c.name,
    description: c.description,
    inputSchema: c.input_schema,
  };
}

export function callableToIntrospectEntry(c: Callable): Record<string, unknown> {
  return {
    command_id: c.command_id,
    name: c.name,
    description: c.description,
    surfaces: [...c.surfaces],
    input_schema: c.input_schema,
    capabilities: { ...c.capabilities },
  };
}

export function buildIntrospectDocument(version: string): Record<string, unknown> {
  return {
    schema: INTROSPECT_SCHEMA_ID,
    binary: BINARY_NAME,
    version,
    callables_declared: true,
    callables: getCallables().map(callableToIntrospectEntry),
    exit_codes: { ...EXIT_CODES },
  };
}
