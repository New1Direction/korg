// Public API for programmatic / library use.

export { EventIndex, type IndexedEvent } from "./event-index.js";
export {
  DEFAULT_EMBEDDING_MODEL,
  DEFAULT_MIN_SCORE,
  DEFAULT_TOP_N,
  EmbeddingDependencyMissing,
  type Match,
  type Mode,
  RecallEngine,
} from "./search.js";
export {
  buildServer,
  formatMatchesForLlm,
  handleRecallCall,
  SERVER_NAME,
  SERVER_VERSION,
  serveStdio,
} from "./server.js";
export {
  BINARY_NAME,
  buildIntrospectDocument,
  callableToIntrospectEntry,
  callableToMcpTool,
  type Callable,
  type Capabilities,
  EXIT_CODES,
  getCallables,
  INTROSPECT_SCHEMA_ID,
} from "./introspect.js";
export { textForEvent, type LedgerEvent } from "./text.js";
