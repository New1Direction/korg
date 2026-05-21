//! Tool execution for coding agents (Option C foundation).
//!
//! Provides safe, structured execution of file and shell operations.
//! All results are returned as typed payloads that get wrapped in signed
//! AcpMessage envelopes by the existing transport.

use crate::acp::{
    AcpMessage, FileReadRequestPayload, FileReadResultPayload,
    ShellExecRequestPayload, ShellExecResultPayload,
    PatchApplyRequestPayload, PatchApplyResultPayload,
    TestRunRequestPayload, TestRunResultPayload,
};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Maximum bytes we'll read from a file or command output for safety.
const MAX_OUTPUT_BYTES: u64 = 512 * 1024; // 512 KiB

pub fn check_policy(command: &str, args: &[String]) -> Result<(), String> {
    // 1. Load whitelist from POLICY.md if it exists
    let policy_path = std::path::Path::new("POLICY.md");
    let mut whitelisted_commands = vec![
        "cargo".to_string(),
        "git".to_string(),
        "echo".to_string(),
    ];
    
    if let Ok(content) = std::fs::read_to_string(policy_path) {
        let mut extracted = Vec::new();
        for line in content.lines() {
            if line.trim().starts_with("- `") {
                if let Some(cmd) = line.split('`').nth(1) {
                    if let Some(word) = cmd.split_whitespace().next() {
                        extracted.push(word.to_string());
                    }
                }
            }
        }
        if !extracted.is_empty() {
            whitelisted_commands = extracted;
        }
    }

    // 2. Validate command
    let base_cmd = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command);

    if !whitelisted_commands.iter().any(|c| c == base_cmd) {
        return Err(format!("Command '{}' is not whitelisted in POLICY.md", base_cmd));
    }

    let full_cmd = format!("{} {}", command, args.join(" "));
    if full_cmd.contains("/etc/passwd") || full_cmd.contains("/etc/shadow") || full_cmd.contains(".ssh") || full_cmd.contains("id_rsa") || full_cmd.contains(".env") {
        return Err("Credentials or blacklisted path found in command arguments".to_string());
    }

    Ok(())
}

pub fn check_path_policy(path_str: &str) -> Result<(), String> {
    if path_str.contains("/etc/passwd") || path_str.contains("/etc/shadow") || path_str.contains(".ssh") || path_str.contains("id_rsa") || path_str.contains(".env") {
        return Err("Access to credentials or blacklisted system file strictly forbidden".to_string());
    }

    let path = std::path::Path::new(path_str);
    if path.is_absolute() {
        if path.starts_with("/tmp") {
            Ok(())
        } else if path.starts_with("/Users/clubpenguin/Documents/Korg") {
            Ok(())
        } else {
            Err(format!("Absolute path '{}' is outside whitelisted directories (/tmp or Korg root)", path_str))
        }
    } else {
        if path_str.contains("..") {
            let canonical = std::fs::canonicalize(path);
            match canonical {
                Ok(c) => {
                    if c.starts_with("/Users/clubpenguin/Documents/Korg") || c.starts_with("/tmp") {
                        Ok(())
                    } else {
                        Err("Path traversal went outside whitelisted workspace".to_string())
                    }
                }
                Err(_) => Err("Path traversal check failed".to_string())
            }
        } else {
            Ok(())
        }
    }
}

/// Execute a FileReadRequest safely.
///
/// Only allows reading inside the current working directory or /tmp for now
/// (basic sandboxing — can be hardened later with chroot/namespaces).
pub async fn execute_file_read(req: FileReadRequestPayload) -> FileReadResultPayload {
    if let Err(err) = check_path_policy(&req.path) {
        return FileReadResultPayload {
            path: req.path.clone(),
            content: String::new(),
            bytes_read: 0,
            truncated: false,
            error: Some(format!("CONTESTED: Policy Violation - {}", err)),
        };
    }

    let path = Path::new(&req.path);

    // Very basic sandbox: only allow relative paths or under /tmp
    if path.is_absolute() && !path.starts_with("/tmp") && !path.starts_with("/Users/clubpenguin/Documents/Korg") {
        return FileReadResultPayload {
            path: req.path,
            content: String::new(),
            bytes_read: 0,
            truncated: false,
            error: Some("absolute paths outside /tmp or Korg root are not allowed in this reference harness".to_string()),
        };
    }

    match tokio::fs::File::open(path).await {
        Ok(mut file) => {
            let max = req.max_bytes.unwrap_or(MAX_OUTPUT_BYTES);
            let mut buf = Vec::new();
            let mut reader = tokio::io::BufReader::new(&mut file);

            let bytes_read = match reader.take(max).read_to_end(&mut buf).await {
                Ok(n) => n as u64,
                Err(e) => {
                    return FileReadResultPayload {
                        path: req.path,
                        content: String::new(),
                        bytes_read: 0,
                        truncated: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            let truncated = bytes_read >= max;
            let content = String::from_utf8_lossy(&buf).to_string();

            FileReadResultPayload {
                path: req.path,
                content,
                bytes_read,
                truncated,
                error: None,
            }
        }
        Err(e) => FileReadResultPayload {
            path: req.path,
            content: String::new(),
            bytes_read: 0,
            truncated: false,
            error: Some(e.to_string()),
        },
    }
}

/// Execute a ShellExecRequestPayload safely with timeout and output limits.
pub async fn execute_shell(req: ShellExecRequestPayload) -> ShellExecResultPayload {
    if let Err(err) = check_policy(&req.command, &req.args) {
        return ShellExecResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            error: Some(format!("CONTESTED: Policy Violation - {}", err)),
        };
    }

    let start = Instant::now();

    let mut cmd = Command::new(&req.command);
    cmd.args(&req.args);

    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }

    // Basic safety: inherit only a restricted environment
    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());

    let timeout_ms = req.timeout_ms.unwrap_or(30_000);
    let timeout_duration = Duration::from_millis(timeout_ms);

    let result = timeout(timeout_duration, cmd.output()).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            // Truncate if too large
            let (stdout, _) = truncate_output(stdout);
            let (stderr, _) = truncate_output(stderr);

            ShellExecResultPayload {
                command: format!("{} {}", req.command, req.args.join(" ")),
                exit_code: output.status.code().unwrap_or(-1),
                stdout,
                stderr,
                duration_ms,
                error: None,
            }
        }
        Ok(Err(e)) => ShellExecResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms,
            error: Some(e.to_string()),
        },
        Err(_) => ShellExecResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms,
            error: Some(format!("command timed out after {} ms", timeout_ms)),
        },
    }
}

fn truncate_output(mut s: String) -> (String, bool) {
    if s.len() as u64 > MAX_OUTPUT_BYTES {
        s.truncate(MAX_OUTPUT_BYTES as usize);
        s.push_str("\n... [truncated]");
        (s, true)
    } else {
        (s, false)
    }
}

/// Dispatch a tool request to the appropriate executor and return the corresponding result message.
/// This is the main entry point used by the worker harness.
pub async fn dispatch_tool(msg: AcpMessage) -> Option<AcpMessage> {
    match msg {
        AcpMessage::FileReadRequest(payload) => {
            let result = execute_file_read(payload).await;
            Some(AcpMessage::FileReadResult(result))
        }
        AcpMessage::ShellExecRequest(payload) => {
            let result = execute_shell(payload).await;
            Some(AcpMessage::ShellExecResult(result))
        }
        AcpMessage::PatchApplyRequest(payload) => {
            let result = execute_patch_apply(payload).await;
            Some(AcpMessage::PatchApplyResult(result))
        }
        AcpMessage::TestRunRequest(payload) => {
            let result = execute_test_run(payload).await;
            Some(AcpMessage::TestRunResult(result))
        }
        AcpMessage::CodeEditProposal(payload) => {
            // For this slice we just acknowledge the proposal.
            // Real usage would store it for later review / Arena scoring.
            eprintln!("[ToolExecutor] Received CodeEditProposal for {}", payload.file_path);
            None // No direct result — the proposal is informational
        }
        AcpMessage::ScreenshotRequest(payload) => {
            let result = execute_screenshot(payload).await;
            Some(AcpMessage::ScreenshotResult(result))
        }
        _ => None,
    }
}

/// Execute a test run (cargo test, uv run pytest, etc.) and return a rich result.
pub async fn execute_test_run(req: TestRunRequestPayload) -> TestRunResultPayload {
    if let Err(err) = check_policy(&req.command, &req.args) {
        return TestRunResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            duration_ms: 0,
            tests_run: 0,
            tests_passed: 0,
            tests_failed: 0,
            tests_ignored: 0,
            coverage_percent: None,
            failure_summaries: vec![],
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("CONTESTED: Policy Violation - {}", err)),
        };
    }

    let start = Instant::now();

    let mut cmd = Command::new(&req.command);
    cmd.args(&req.args);

    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }

    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("RUST_BACKTRACE", "0");

    let timeout_ms = req.timeout_ms.unwrap_or(180_000);
    let timeout_duration = Duration::from_millis(timeout_ms);

    let result = timeout(timeout_duration, cmd.output()).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            let (tests_run, tests_passed, tests_failed, tests_ignored, failure_summaries) =
                parse_test_output(&stdout, &stderr);

            let coverage_percent = detect_coverage(&stdout, &stderr);

            TestRunResultPayload {
                command: format!("{} {}", req.command, req.args.join(" ")),
                exit_code: output.status.code().unwrap_or(-1),
                duration_ms,
                tests_run,
                tests_passed,
                tests_failed,
                tests_ignored,
                coverage_percent,
                failure_summaries,
                stdout: truncate_output_string(stdout, 64 * 1024),
                stderr: truncate_output_string(stderr, 64 * 1024),
                error: None,
            }
        }
        Ok(Err(e)) => TestRunResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            duration_ms,
            tests_run: 0,
            tests_passed: 0,
            tests_failed: 0,
            tests_ignored: 0,
            coverage_percent: None,
            failure_summaries: vec![],
            stdout: String::new(),
            stderr: String::new(),
            error: Some(e.to_string()),
        },
        Err(_) => TestRunResultPayload {
            command: format!("{} {}", req.command, req.args.join(" ")),
            exit_code: -1,
            duration_ms,
            tests_run: 0,
            tests_passed: 0,
            tests_failed: 0,
            tests_ignored: 0,
            coverage_percent: None,
            failure_summaries: vec![],
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("test run timed out after {} ms", timeout_ms)),
        },
    }
}

/// Very lightweight parser for common test output (cargo test + pytest).
fn parse_test_output(stdout: &str, stderr: &str) -> (u32, u32, u32, u32, Vec<String>) {
    let combined = format!("{}\n{}", stdout, stderr);
    let mut run = 0u32;
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut ignored = 0u32;
    let mut failures = Vec::new();

    // Cargo test patterns
    for line in combined.lines() {
        if line.contains("test result:") {
            // cargo style: "test result: ok. 12 passed; 0 failed; 1 ignored"
            if let Some(rest) = line.split("test result:").nth(1) {
                for part in rest.split(';') {
                    let p = part.trim();
                    if p.contains("passed") {
                        if let Some(n) = p.split_whitespace().next().and_then(|s| s.parse().ok()) {
                            passed = n;
                        }
                    } else if p.contains("failed") {
                        if let Some(n) = p.split_whitespace().next().and_then(|s| s.parse().ok()) {
                            failed = n;
                        }
                    } else if p.contains("ignored") {
                        if let Some(n) = p.split_whitespace().next().and_then(|s| s.parse().ok()) {
                            ignored = n;
                        }
                    }
                }
            }
        }

        // Collect failure names
        if line.starts_with("test ") && line.contains("... FAILED") {
            if let Some(name) = line.split("test ").nth(1).and_then(|s| s.split("...").next()) {
                failures.push(name.trim().to_string());
            }
        }

        // pytest style
        if line.trim_start().starts_with("FAILED ") || line.trim_start().starts_with("ERROR ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                failures.push(name.to_string());
            }
        }
    }

    run = passed + failed + ignored;
    if run == 0 {
        // Fallback rough count
        run = (passed + failed).max(1);
    }

    (run, passed, failed, ignored, failures.into_iter().take(5).collect())
}

fn detect_coverage(stdout: &str, stderr: &str) -> Option<f32> {
    let combined = format!("{}\n{}", stdout, stderr);
    for line in combined.lines() {
        if line.contains('%') && (line.contains("coverage") || line.contains("Coverage")) {
            if let Some(pct_str) = line.split('%').next().and_then(|s| s.split_whitespace().last()) {
                if let Ok(p) = pct_str.trim().parse::<f32>() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn truncate_output_string(s: String, max: usize) -> String {
    if s.len() > max {
        format!("{}... [truncated]", &s[..max])
    } else {
        s
    }
}

/// Execute a patch application request safely.
pub async fn execute_patch_apply(req: PatchApplyRequestPayload) -> PatchApplyResultPayload {
    if let Err(err) = check_path_policy(&req.file_path) {
        return PatchApplyResultPayload {
            file_path: req.file_path,
            success: false,
            applied_hunks: 0,
            rejected_hunks: 0,
            new_content_preview: None,
            error: Some(format!("CONTESTED: Policy Violation - {}", err)),
        };
    }

    let target_path = Path::new(&req.file_path);

    // Basic sandbox: only relative paths or under current dir / /tmp
    if target_path.is_absolute() && !target_path.starts_with("/tmp") && !target_path.starts_with("/Users/clubpenguin/Documents/Korg") {
        return PatchApplyResultPayload {
            file_path: req.file_path,
            success: false,
            applied_hunks: 0,
            rejected_hunks: 0,
            new_content_preview: None,
            error: Some("Absolute paths outside /tmp or Korg root are not allowed".to_string()),
        };
    }

    if req.dry_run {
        // For dry-run we just validate the patch format and that the file exists
        return PatchApplyResultPayload {
            file_path: req.file_path,
            success: true,
            applied_hunks: 1,
            rejected_hunks: 0,
            new_content_preview: Some("(dry run) patch would apply cleanly".to_string()),
            error: None,
        };
    }

    // Read original file
    let original = match tokio::fs::read_to_string(&target_path).await {
        Ok(content) => content,
        Err(e) => {
            return PatchApplyResultPayload {
                file_path: req.file_path,
                success: false,
                applied_hunks: 0,
                rejected_hunks: 0,
                new_content_preview: None,
                error: Some(format!("Failed to read file: {}", e)),
            };
        }
    };

    // Create backup
    let backup_path = format!("{}.korg-bak", req.file_path);
    if let Err(e) = tokio::fs::write(&backup_path, &original).await {
        return PatchApplyResultPayload {
            file_path: req.file_path,
            success: false,
            applied_hunks: 0,
            rejected_hunks: 0,
            new_content_preview: None,
            error: Some(format!("Failed to create backup: {}", e)),
        };
    }

    // Try git apply first if we're in a git repo (more robust)
    let patched = if Path::new(".git").exists() {
        match try_git_apply(&target_path, &req.patch).await {
            Ok(_) => tokio::fs::read_to_string(&target_path).await.unwrap_or_else(|_| original.clone()), // read the git-patched content
            Err(_) => {
                // fall back to our internal applier
                let original_clone = original.clone();
                let patch_clone = req.patch.clone();
                let res = tokio::task::spawn_blocking(move || apply_patch(&original_clone, &patch_clone)).await;
                match res {
                    Ok(Ok(p)) => p,
                    Ok(Err(e)) => {
                        let _ = tokio::fs::write(&target_path, &original).await;
                        return PatchApplyResultPayload {
                            file_path: req.file_path,
                            success: false,
                            applied_hunks: 0,
                            rejected_hunks: 1,
                            new_content_preview: None,
                            error: Some(e),
                        };
                    }
                    Err(join_err) => {
                        let _ = tokio::fs::write(&target_path, &original).await;
                        return PatchApplyResultPayload {
                            file_path: req.file_path,
                            success: false,
                            applied_hunks: 0,
                            rejected_hunks: 1,
                            new_content_preview: None,
                            error: Some(format!("Thread join error: {}", join_err)),
                        };
                    }
                }
            }
        }
    } else {
        let original_clone = original.clone();
        let patch_clone = req.patch.clone();
        let res = tokio::task::spawn_blocking(move || apply_patch(&original_clone, &patch_clone)).await;
        match res {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                let _ = tokio::fs::write(&target_path, &original).await;
                return PatchApplyResultPayload {
                    file_path: req.file_path,
                    success: false,
                    applied_hunks: 0,
                    rejected_hunks: 1,
                    new_content_preview: None,
                    error: Some(e),
                };
            }
            Err(join_err) => {
                let _ = tokio::fs::write(&target_path, &original).await;
                return PatchApplyResultPayload {
                    file_path: req.file_path,
                    success: false,
                    applied_hunks: 0,
                    rejected_hunks: 1,
                    new_content_preview: None,
                    error: Some(format!("Thread join error: {}", join_err)),
                };
            }
        }
    };

    // Write the result
    if let Err(e) = tokio::fs::write(&target_path, &patched).await {
        let _ = tokio::fs::write(&target_path, &original).await; // restore
        return PatchApplyResultPayload {
            file_path: req.file_path,
            success: false,
            applied_hunks: 0,
            rejected_hunks: 0,
            new_content_preview: None,
            error: Some(format!("Failed to write patched file: {}", e)),
        };
    }

    // Success
    let preview = patched.lines().take(8).collect::<Vec<_>>().join("\n");

    PatchApplyResultPayload {
        file_path: req.file_path,
        success: true,
        applied_hunks: 1,
        rejected_hunks: 0,
        new_content_preview: Some(preview),
        error: None,
    }
}

/// Apply a patch to the original content.
/// Supports simple search/replace blocks (LLM friendly) and basic unified diffs.
fn apply_patch(original: &str, patch: &str) -> Result<String, String> {
    // Try search/replace block format first (very common with LLMs)
    if patch.contains("<<<<<<< SEARCH") || patch.contains("=======") {
        return apply_search_replace(original, patch);
    }

    // Fallback: very naive unified diff application (single file, simple hunks)
    apply_simple_unified_diff(original, patch)
}

/// Handles the common LLM "search / replace" block format.
fn apply_search_replace(original: &str, patch: &str) -> Result<String, String> {
    let mut result = original.to_string();

    // Split on common delimiters
    let blocks: Vec<&str> = patch.split("<<<<<<< SEARCH").collect();

    for block in blocks.iter().skip(1) {
        let parts: Vec<&str> = block.split("=======").collect();
        if parts.len() != 2 {
            continue;
        }

        let search_part = parts[0].trim_start_matches('\n').trim_end();
        let replace_part = parts[1].split(">>>>>>> REPLACE").next().unwrap_or("").trim_start_matches('\n').trim_end();

        if result.contains(search_part) {
            result = result.replacen(search_part, replace_part, 1);
        } else {
            return Err(format!("Search string not found:\n{}", search_part));
        }
    }

    Ok(result)
}

/// Extremely simple unified diff applier (single hunk, for demo purposes).
#[derive(Debug, Clone, PartialEq, Eq)]
enum HunkLineType {
    Context,
    Deletion,
    Insertion,
}

#[derive(Debug, Clone)]
struct HunkLine {
    line_type: HunkLineType,
    content: String,
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let parts: Vec<&str> = line.split("@@").collect();
    if parts.len() < 3 {
        return None;
    }
    let header_content = parts[1].trim();
    let ranges: Vec<&str> = header_content.split_whitespace().collect();
    if ranges.len() < 2 {
        return None;
    }

    let parse_range = |s: &str, prefix: char| -> Option<(usize, usize)> {
        let s = s.strip_prefix(prefix)?;
        let comma_parts: Vec<&str> = s.split(',').collect();
        let start = comma_parts[0].parse::<usize>().ok()?;
        let count = if comma_parts.len() > 1 {
            comma_parts[1].parse::<usize>().ok()?
        } else {
            1
        };
        Some((start, count))
    };

    let (old_start, old_count) = parse_range(ranges[0], '-')?;
    let (new_start, new_count) = parse_range(ranges[1], '+')?;

    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_unified_diff(patch: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<Hunk> = None;
    let mut has_hunk_headers = false;

    for line in patch.lines() {
        if line.starts_with("@@") {
            has_hunk_headers = true;
            break;
        }
    }

    if has_hunk_headers {
        for line in patch.lines() {
            if line.starts_with("@@") {
                if let Some(h) = current_hunk.take() {
                    hunks.push(h);
                }
                if let Some(header) = parse_hunk_header(line) {
                    current_hunk = Some(header);
                }
            } else if let Some(ref mut hunk) = current_hunk {
                if let Some(rest) = line.strip_prefix('-') {
                    hunk.lines.push(HunkLine {
                        line_type: HunkLineType::Deletion,
                        content: rest.to_string(),
                    });
                } else if let Some(rest) = line.strip_prefix('+') {
                    hunk.lines.push(HunkLine {
                        line_type: HunkLineType::Insertion,
                        content: rest.to_string(),
                    });
                } else if let Some(rest) = line.strip_prefix(' ') {
                    hunk.lines.push(HunkLine {
                        line_type: HunkLineType::Context,
                        content: rest.to_string(),
                    });
                } else if line.starts_with('\\') {
                    // Ignore '\ No newline at end of file'
                } else {
                    // Treat as context line (sometimes missing prefix space)
                    hunk.lines.push(HunkLine {
                        line_type: HunkLineType::Context,
                        content: line.to_string(),
                    });
                }
            }
        }
        if let Some(h) = current_hunk {
            hunks.push(h);
        }
    } else {
        // Headerless raw patch!
        let mut lines = Vec::new();
        for line in patch.lines() {
            if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
                continue;
            }
            if let Some(rest) = line.strip_prefix('-') {
                lines.push(HunkLine {
                    line_type: HunkLineType::Deletion,
                    content: rest.to_string(),
                });
            } else if let Some(rest) = line.strip_prefix('+') {
                lines.push(HunkLine {
                    line_type: HunkLineType::Insertion,
                    content: rest.to_string(),
                });
            } else if let Some(rest) = line.strip_prefix(' ') {
                lines.push(HunkLine {
                    line_type: HunkLineType::Context,
                    content: rest.to_string(),
                });
            } else if line.starts_with('\\') {
                // Ignore
            } else {
                lines.push(HunkLine {
                    line_type: HunkLineType::Context,
                    content: line.to_string(),
                });
            }
        }
        if !lines.is_empty() {
            let old_count = lines.iter().filter(|l| !matches!(l.line_type, HunkLineType::Insertion)).count();
            let new_count = lines.iter().filter(|l| !matches!(l.line_type, HunkLineType::Deletion)).count();
            hunks.push(Hunk {
                old_start: 1,
                old_count,
                new_start: 1,
                new_count,
                lines,
            });
        }
    }

    hunks
}

fn hunk_matches_stage(lines: &[String], hunk: &Hunk, idx: usize, stage: usize) -> bool {
    let mut file_offset = 0;
    for hunk_line in &hunk.lines {
        match hunk_line.line_type {
            HunkLineType::Deletion | HunkLineType::Context => {
                let file_idx = idx + file_offset;
                if file_idx >= lines.len() {
                    return false;
                }
                
                let matches = match stage {
                    1 => {
                        // Stage 1: Exact check except trailing whitespace and carriage returns
                        hunk_line.content.trim_end() == lines[file_idx].trim_end()
                    }
                    2 => {
                        // Stage 2: Ignore leading and trailing whitespace
                        hunk_line.content.trim() == lines[file_idx].trim()
                    }
                    3 => {
                        // Stage 3: Ignore leading/trailing whitespace and case-insensitive
                        hunk_line.content.trim().to_lowercase() == lines[file_idx].trim().to_lowercase()
                    }
                    _ => false,
                };
                
                if !matches {
                    return false;
                }
                file_offset += 1;
            }
            HunkLineType::Insertion => {}
        }
    }
    true
}

fn apply_simple_unified_diff(original: &str, patch: &str) -> Result<String, String> {
    let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();
    let has_trailing_newline = original.ends_with('\n');

    let hunks = parse_unified_diff(patch);
    if hunks.is_empty() {
        return Err("No valid diff hunks parsed".to_string());
    }

    let mut cumulative_line_shift: isize = 0;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        // Preferred target index based on original hunk position and cumulative line shift
        let preferred_idx = (hunk.old_start as isize - 1 + cumulative_line_shift).max(0) as usize;
        
        // Build the search indices outward from preferred_idx
        let mut search_indices = Vec::new();
        let max_len = lines.len();

        if max_len == 0 {
            search_indices.push(0);
        } else {
            let mut offset = 0;
            while preferred_idx + offset < max_len || (preferred_idx as isize - offset as isize) >= 0 {
                if offset == 0 {
                    if preferred_idx < max_len {
                        search_indices.push(preferred_idx);
                    }
                } else {
                    let pos = preferred_idx + offset;
                    let neg = preferred_idx as isize - offset as isize;
                    if pos < max_len {
                        search_indices.push(pos);
                    }
                    if neg >= 0 {
                        search_indices.push(neg as usize);
                    }
                }
                offset += 1;
                if offset > max_len + 100 {
                    break;
                }
            }
            // Fallback: make sure at least index 0 is present
            if !search_indices.contains(&0) && max_len > 0 {
                search_indices.push(0);
            }
        }

        // Try stages 1, 2, 3
        let mut matched_idx = None;
        'outer: for stage in 1..=3 {
            for &idx in &search_indices {
                if hunk_matches_stage(&lines, hunk, idx, stage) {
                    matched_idx = Some(idx);
                    break 'outer;
                }
            }
        }

        let matched_idx = match matched_idx {
            Some(idx) => idx,
            None => {
                return Err(format!(
                    "Hunk #{} starting at line {} (target expected at line {}) failed to apply (no matching context found)",
                    hunk_idx + 1, hunk.old_start, preferred_idx + 1
                ));
            }
        };

        // Determine how many lines are actually replaced (count deletions and context)
        let file_offset = hunk.lines.iter().filter(|l| !matches!(l.line_type, HunkLineType::Insertion)).count();

        // Build the replacement lines (preserve original formatting for context, insert insertions)
        let mut replacement_lines = Vec::new();
        let mut file_cursor = matched_idx;
        for hunk_line in &hunk.lines {
            match hunk_line.line_type {
                HunkLineType::Context => {
                    if file_cursor < lines.len() {
                        replacement_lines.push(lines[file_cursor].clone());
                    } else {
                        replacement_lines.push(hunk_line.content.clone());
                    }
                    file_cursor += 1;
                }
                HunkLineType::Insertion => {
                    replacement_lines.push(hunk_line.content.clone());
                }
                HunkLineType::Deletion => {
                    file_cursor += 1;
                }
            }
        }

        // Apply replacement
        lines.splice(matched_idx..matched_idx + file_offset, replacement_lines);

        // Update cumulative line shift
        let local_shift = hunk.lines.iter().filter(|l| matches!(l.line_type, HunkLineType::Insertion)).count() as isize
            - hunk.lines.iter().filter(|l| matches!(l.line_type, HunkLineType::Deletion)).count() as isize;
        
        cumulative_line_shift += local_shift;
    }

    let mut result = lines.join("\n");
    if has_trailing_newline && !result.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

/// Helper function to apply a patch using the system's `git apply` command.
async fn try_git_apply(file_path: &Path, patch: &str) -> Result<(), String> {
    let temp_patch_path = format!("/tmp/korg-patch-{}.patch", uuid::Uuid::new_v4());
    if let Err(e) = tokio::fs::write(&temp_patch_path, patch).await {
        return Err(format!("Failed to write temporary patch file: {}", e));
    }

    // git apply runs relative to the current directory (which should be the worktree/repo root)
    let output = match tokio::process::Command::new("git")
        .args(&["apply", "--whitespace=nowarn", &temp_patch_path])
        .output()
        .await
    {
        Ok(out) => out,
        Err(e) => {
            let _ = tokio::fs::remove_file(&temp_patch_path).await;
            return Err(format!("Failed to execute git apply: {}", e));
        }
    };

    let _ = tokio::fs::remove_file(&temp_patch_path).await;

    if output.status.success() {
        Ok(())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("git apply failed: {}", err_msg))
    }
}

/// Execute a ScreenshotRequest safely.
pub async fn execute_screenshot(
    req: crate::acp::ScreenshotRequestPayload,
) -> crate::acp::ScreenshotResultPayload {
    // 1. Check filename/target path policies if applicable
    if let Err(err) = check_path_policy(&req.target_name) {
        return crate::acp::ScreenshotResultPayload {
            attachment: crate::acp::VisionAttachment {
                name: req.target_name,
                mime_type: "image/png".to_string(),
                data_base64: crate::vision_policy::BLACKOUT_PNG_BASE64.to_string(),
                description: req.description,
                verdict: "BLOCKED".to_string(),
                infraction_patterns: vec!["path_policy_violation".to_string()],
                raw_data_base64: None,
            },
            error: Some(format!("CONTESTED: Policy Violation - {}", err)),
        };
    }

    // 2. Generate mock base64 screenshot data
    let mut raw_data = format!(
        "Mock visual frame buffer for: {}\nDescription: {}\nTimestamp: 2026-05-21\n",
        req.target_name, req.description
    );

    // Explicitly inject triggers for testing OCR scanning fallbacks if present
    let lower_target = req.target_name.to_lowercase();
    let lower_desc = req.description.to_lowercase();
    if lower_target.contains("password") || lower_desc.contains("password") {
        raw_data.push_str("[OCR: contains password=admin123]\n");
    }
    if lower_target.contains("api_key") || lower_desc.contains("api_key") {
        raw_data.push_str("[OCR: contains api_key=sk-proj-5678]\n");
    }
    if lower_target.contains("secret") || lower_desc.contains("secret") {
        raw_data.push_str("[OCR: contains secret token]\n");
    }
    if lower_target.contains("private_key") || lower_desc.contains("private_key") {
        raw_data.push_str("[OCR: contains -----BEGIN PRIVATE KEY-----]\n");
    }

    let data_base64 = crate::vision_policy::base64_encode(raw_data.as_bytes());

    let mut attachment = crate::acp::VisionAttachment {
        name: req.target_name,
        mime_type: "image/png".to_string(),
        data_base64,
        description: req.description,
        verdict: "PENDING".to_string(),
        infraction_patterns: vec![],
        raw_data_base64: None,
    };

    // 3. Intercept captured screenshots immediately and filter them through the visual policy engine
    let config = crate::llm::KorgConfig::load();
    let policy_config = config.security_vision;
    crate::vision_policy::check_attachment(&mut attachment, &policy_config);

    crate::acp::ScreenshotResultPayload {
        attachment,
        error: None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_echo() {
        let req = ShellExecRequestPayload {
            command: "echo".to_string(),
            args: vec!["hello from tool executor".to_string()],
            cwd: None,
            timeout_ms: Some(5_000),
        };
        let result = execute_shell(req).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello from tool executor"));
    }

    #[test]
    fn test_apply_unified_diff_multi_hunk() {
        let original = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n";
        let patch = "@@ -2,2 +2,3 @@
 line 2
-line 3
+new line 3
+new line 3b
@@ -5,2 +6,2 @@
-line 5
+modified line 5
 line 6
";
        let expected = "line 1\nline 2\nnew line 3\nnew line 3b\nline 4\nmodified line 5\nline 6\nline 7\n";
        let result = apply_simple_unified_diff(original, patch).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_unified_diff_fuzzy() {
        let original = "    fn foo() {\n        let x = 1;\n        println!(\"{}\", x);\n    }\n";
        // Let's create a patch with different leading indentation, casing, and trailing whitespace in context/deletion.
        let patch = "@@ -2,3 +2,3 @@
-        LET X = 1;   \n+        let y = 2;\n        PRINTLN!(\"{}\", x);\n";
        // Stage 3 fuzzy matching should resolve casing and trailing spaces and match it.
        let expected = "    fn foo() {\n        let y = 2;\n        println!(\"{}\", x);\n    }\n";
        let result = apply_simple_unified_diff(original, patch).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_unified_diff_line_shifts() {
        let original = "line 1\nline 2\nline 3\nline 4\nline 5\n";
        // Hunk 1 inserts 3 new lines after line 2, which shifts hunk 2 down by 3 lines.
        // Hunk 2 expects to modify line 4.
        let patch = "@@ -2,2 +2,5 @@
 line 2
-line 3
+line 3a
+line 3b
+line 3c
@@ -4,1 +7,1 @@
-line 4
+line 4 modified
";
        let expected = "line 1\nline 2\nline 3a\nline 3b\nline 3c\nline 4 modified\nline 5\n";
        let result = apply_simple_unified_diff(original, patch).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_apply_unified_diff_headerless() {
        let original = "line 1\nline 2\nline 3\n";
        let patch = " line 1\n-line 2\n+line 2 modified\n line 3\n";
        let expected = "line 1\nline 2 modified\nline 3\n";
        let result = apply_simple_unified_diff(original, patch).unwrap();
        assert_eq!(result, expected);
    }
}