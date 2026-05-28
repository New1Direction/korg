"""korg-setup — one-command install for the korg Claude Code loop."""

from korg_setup.claude_config import (
    DEFAULT_CONFIG_PATH,
    McpServerSpec,
    ensure_mcp_server_registered,
    get_registered_server,
    load_config,
    remove_mcp_server,
    save_config_atomic,
)
from korg_setup.launchd import (
    LABEL,
    LOG_DIR,
    PLIST_PATH,
    PlistSpec,
    UnsupportedPlatformError,
    build_spec,
    install_service,
    is_loaded,
    is_macos,
    uninstall_service,
    write_plist,
)
from korg_setup.setup import (
    DEFAULT_LEDGER_DIR,
    DEFAULT_LEDGER_FILE,
    DEFAULT_MCP_SERVER_NAME,
    DEFAULT_TAIL_STATE,
    SetupReport,
    SetupStep,
    format_report,
    run_setup,
)
from korg_setup.status import StatusReport, format_status, gather_status

__version__ = "0.1.0"

__all__ = [
    # claude_config
    "DEFAULT_CONFIG_PATH",
    "McpServerSpec",
    "ensure_mcp_server_registered",
    "get_registered_server",
    "load_config",
    "remove_mcp_server",
    "save_config_atomic",
    # launchd
    "LABEL",
    "LOG_DIR",
    "PLIST_PATH",
    "PlistSpec",
    "UnsupportedPlatformError",
    "build_spec",
    "install_service",
    "is_loaded",
    "is_macos",
    "uninstall_service",
    "write_plist",
    # setup
    "DEFAULT_LEDGER_DIR",
    "DEFAULT_LEDGER_FILE",
    "DEFAULT_MCP_SERVER_NAME",
    "DEFAULT_TAIL_STATE",
    "SetupReport",
    "SetupStep",
    "format_report",
    "run_setup",
    # status
    "StatusReport",
    "format_status",
    "gather_status",
]
