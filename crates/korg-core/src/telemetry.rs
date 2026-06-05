//! Korg Telemetry — Structured Tracing Initialization
//!
//! Initializes a `tracing-subscriber` stack that respects the `KORG_LOG` environment variable
//! (defaults to `info`). Set `KORG_LOG_JSON=1` for machine-parseable JSON output.
//!
//! # Usage
//!
//! ```text
//! KORG_LOG=debug cargo run -- campaign
//! KORG_LOG=info,korg=debug,korg::registry=trace cargo run -- campaign
//! KORG_LOG_JSON=1 KORG_LOG=info cargo run -- --web "fix auth module"
//! ```

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Whether the given process args will launch the full-screen TUI, in which case
/// tracing must NOT write to the terminal (it would render on top of ratatui).
///
/// True for the `tui` subcommand or any `--tui` flag; an explicit `--headless`
/// always wins (false). Pure + arg-driven so it's testable without a real argv.
pub fn args_launch_tui(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--headless") {
        return false;
    }
    args.iter().any(|a| a == "tui" || a == "--tui")
}

/// Initialize tracing, routing output to a log FILE instead of the terminal when
/// `args` indicate the TUI will take over the screen. Returns the log path if one
/// was opened, so the caller can tell the user where logs went.
pub fn init_tracing_for(args: &[String]) -> Option<std::path::PathBuf> {
    if args_launch_tui(args) {
        let dir = crate::paths::cache_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("korg-tui.log");
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let env_filter =
                EnvFilter::try_from_env("KORG_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
            let file_layer = fmt::layer()
                .with_ansi(false) // never write escape codes into a log file
                .with_writer(file)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);
            tracing_subscriber::registry()
                .with(env_filter)
                .with(file_layer)
                .try_init()
                .ok();
            return Some(path);
        }
    }
    init_tracing();
    None
}

/// Initialize the global tracing subscriber.
///
/// Must be called exactly once, at the top of `main()`, before any async tasks are spawned.
/// Subsequent calls (e.g. in tests) are silently no-ops via `try_init`.
pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_env("KORG_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let use_json = std::env::var("KORG_LOG_JSON")
        .map(|v| v == "1")
        .unwrap_or(false);

    if use_json {
        // Machine-readable JSON for log shipping / structured analysis
        let json_layer = fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_target(true)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(json_layer)
            .try_init()
            .ok();
    } else {
        // Human-readable pretty output for development
        let pretty_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()));

        tracing_subscriber::registry()
            .with(env_filter)
            .with(pretty_layer)
            .try_init()
            .ok();
    }
}

/// Convenience macro: emit a structured event with a capability transition context.
/// Example: `trace_transition!("cognition_mode", "Balanced", "Heavy", "Applied")`
#[macro_export]
macro_rules! trace_transition {
    ($capability:expr, $from:expr, $to:expr, $status:expr) => {
        tracing::info!(
            capability = $capability,
            from = ?$from,
            to = ?$to,
            status = $status,
            "capability_transition"
        )
    };
}

/// Convenience macro: emit a structured campaign round event.
/// Example: `trace_round!(3, "Captain", 0.92, "scale_up")`
#[macro_export]
macro_rules! trace_round {
    ($round:expr, $winner:expr, $confidence:expr, $action:expr) => {
        tracing::info!(
            round = $round,
            arena_winner = $winner,
            arena_confidence = $confidence,
            leader_action = $action,
            "campaign_round"
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_is_idempotent() {
        // Multiple init calls should not panic (try_init silently fails on second call)
        init_tracing();
        init_tracing();
    }

    #[test]
    fn tui_subcommand_routes_logs_off_the_terminal() {
        assert!(args_launch_tui(&["korg".into(), "tui".into()]));
    }

    #[test]
    fn tui_flag_routes_logs_off_the_terminal() {
        assert!(args_launch_tui(&["korg".into(), "campaign".into(), "--tui".into()]));
    }

    #[test]
    fn headless_keeps_logs_on_the_terminal() {
        // Explicit headless wins even if --tui is also present.
        assert!(!args_launch_tui(&["korg".into(), "campaign".into()]));
        assert!(!args_launch_tui(&["korg".into(), "--tui".into(), "--headless".into()]));
    }

    #[test]
    fn test_trace_macros_compile() {
        init_tracing();
        trace_transition!("cognition_mode", "Balanced", "Heavy", "Applied");
        trace_round!(1, "Captain", 0.92_f32, "scale_up");
    }
}
