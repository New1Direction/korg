"""korg-recall-mcp — cross-session semantic recall MCP server over the korg ledger."""

from korg_recall_mcp.index import EventIndex, IndexedEvent
from korg_recall_mcp.search import (
    DEFAULT_EMBEDDING_MODEL,
    DEFAULT_MIN_SCORE,
    DEFAULT_TOP_N,
    EmbeddingDependencyMissing,
    Match,
    RecallEngine,
)
from korg_recall_mcp.server import (
    PROTOCOL_VERSION,
    SERVER_NAME,
    SERVER_VERSION,
    Server,
    format_matches_for_llm,
    serve_stdio,
)
from korg_recall_mcp.text import text_for_event

__version__ = "0.1.0"

__all__ = [
    # text
    "text_for_event",
    # index
    "EventIndex",
    "IndexedEvent",
    # search
    "DEFAULT_EMBEDDING_MODEL",
    "DEFAULT_MIN_SCORE",
    "DEFAULT_TOP_N",
    "EmbeddingDependencyMissing",
    "Match",
    "RecallEngine",
    # server
    "PROTOCOL_VERSION",
    "SERVER_NAME",
    "SERVER_VERSION",
    "Server",
    "format_matches_for_llm",
    "serve_stdio",
]
