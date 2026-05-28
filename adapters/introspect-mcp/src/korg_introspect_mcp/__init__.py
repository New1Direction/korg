"""korg-introspect-mcp — generic --introspect → MCP server bridge."""

from korg_introspect_mcp.args import build_argv, kebab
from korg_introspect_mcp.discovery import (
    DISCOVERY_TIMEOUT_S,
    SUPPORTED_SCHEMA,
    DiscoveredBinary,
    DiscoveredCallable,
    DiscoveryError,
    discover,
    resolve_binary,
    run_introspect,
    validate_document,
)
from korg_introspect_mcp.invoker import (
    DEFAULT_TIMEOUT_S,
    InvocationResult,
    invoke,
)
from korg_introspect_mcp.safety import (
    ALL_EFFECTS,
    ALWAYS_ALLOWED,
    Policy,
)
from korg_introspect_mcp.server import (
    PROTOCOL_VERSION,
    SERVER_NAME,
    SERVER_VERSION,
    Server,
    serve_stdio,
)

__version__ = "0.1.0"

__all__ = [
    # args
    "build_argv",
    "kebab",
    # discovery
    "DISCOVERY_TIMEOUT_S",
    "SUPPORTED_SCHEMA",
    "DiscoveredBinary",
    "DiscoveredCallable",
    "DiscoveryError",
    "discover",
    "resolve_binary",
    "run_introspect",
    "validate_document",
    # invoker
    "DEFAULT_TIMEOUT_S",
    "InvocationResult",
    "invoke",
    # safety
    "ALL_EFFECTS",
    "ALWAYS_ALLOWED",
    "Policy",
    # server
    "PROTOCOL_VERSION",
    "SERVER_NAME",
    "SERVER_VERSION",
    "Server",
    "serve_stdio",
]
