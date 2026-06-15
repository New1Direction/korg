//! agent.rs — The real agentic tool-use loop for Korg.
//!
//! This module implements a proper agent loop:
//!   1. Build context from the workspace (files, codebase structure)
//!   2. Send the user prompt + context + tool definitions to the LLM
//!   3. Parse tool calls from the LLM response
//!   4. Execute tools (read files, edit files, run shell commands)
//!   5. Feed results back to the LLM
//!   6. Repeat until the LLM signals completion
//!
//! This is what makes `korg "fix the auth module"` actually work.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use korg_llm::{LlmProvider, LlmRequest, LlmResponse, Message, Role, ToolCall, ToolDefinition};

pub static GLOBAL_CANCELLATION: std::sync::OnceLock<tokio_util::sync::CancellationToken> =
    std::sync::OnceLock::new();

pub fn get_cancellation_token() -> &'static tokio_util::sync::CancellationToken {
    GLOBAL_CANCELLATION.get_or_init(|| tokio_util::sync::CancellationToken::new())
}

// =========================================================================
// Configuration
// =========================================================================

const MAX_AGENT_TURNS: usize = 40;
const MAX_FILE_READ_BYTES: usize = 60_000;
const CONTEXT_SCAN_DEPTH: usize = 3;

// =========================================================================
// Tool Definitions (what the LLM can call)
// =========================================================================

fn agent_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file at the given path. Use this to understand existing code before making changes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative or absolute file path to read"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file, creating it if it doesn't exist or overwriting if it does. Use for creating new files or completely replacing file contents.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to write to"
                    },
                    "content": {
                        "type": "string",
                        "description": "The complete file content to write"
                    }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Apply a targeted edit to an existing file by replacing a specific string with new content. More surgical than write_file — use this for modifying existing files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact string to find and replace (must match exactly, including whitespace)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement string"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Execute a shell command and return stdout/stderr. Use for running tests, builds, linters, git commands, etc.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute (e.g. 'cargo test', 'grep -rn foo src/')"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Optional working directory for the command"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "list_directory".to_string(),
            description: "List files and directories at the given path. Use to understand project structure.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list (default: current directory)"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "If true, list recursively up to 3 levels deep"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "search_files".to_string(),
            description: "Search for a pattern across files using grep. Returns matching lines with file paths and line numbers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The search pattern (supports basic regex)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search in (default: current directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "File glob pattern to include (e.g. '*.rs', '*.py')"
                    }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "task_complete".to_string(),
            description: "Signal that the task is complete. Call this when you have finished all work. Provide a summary of what was done.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "A brief summary of all changes made"
                    }
                },
                "required": ["summary"]
            }),
        },
        ToolDefinition {
            name: "find_symbols".to_string(),
            description: "Extract high-density syntax-aware symbols (functions, structs, classes, implementations, traits, and modules) from a file. Use this to quickly map the structure of a file without reading all its lines.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to scan (must be Rust .rs or Python .py file)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "verify_syntax".to_string(),
            description: "Perform a pre-flight syntax check on a Rust or Python file. Checks for syntax anomalies, errors, and missing elements without compiling. Use this to verify your changes are syntactically sound before testing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to verify"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "query_ast".to_string(),
            description: "Execute a Tree-sitter S-expression query against a file to match exact syntactic structures. Returns matched texts, capture names, and line ranges. Extremely precise for locating nested patterns.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to query"
                    },
                    "query": {
                        "type": "string",
                        "description": "Tree-sitter Lisp-style S-expression query (e.g. '(function_item name: (identifier) @name)')"
                    }
                },
                "required": ["path", "query"]
            }),
        },
        ToolDefinition {
            name: "semantic_search".to_string(),
            description: "Search the codebase index database using semantic vector similarity search. Returns structural code blocks similar to the search query.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Semantic query describing the functional block or concept to search for"
                    },
                    "top_n": {
                        "type": "integer",
                        "description": "Optional number of results to return (default: 3)"
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}

// =========================================================================
// Tool Execution
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolResult {
    success: bool,
    output: String,
}

async fn execute_tool(name: &str, arguments: &str) -> ToolResult {
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to parse tool arguments: {}", e),
            }
        }
    };

    match name {
        "read_file" => execute_read_file(&args).await,
        "write_file" => execute_write_file(&args).await,
        "edit_file" => execute_edit_file(&args).await,
        "run_command" => execute_run_command(&args).await,
        "list_directory" => execute_list_directory(&args).await,
        "search_files" => execute_search_files(&args).await,
        "find_symbols" => execute_find_symbols(&args).await,
        "verify_syntax" => execute_verify_syntax(&args).await,
        "query_ast" => execute_query_ast(&args).await,
        "semantic_search" => execute_semantic_search(&args).await,
        "task_complete" => ToolResult {
            success: true,
            output: args
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Task complete.")
                .to_string(),
        },
        _ => ToolResult {
            success: false,
            output: format!("Unknown tool: {}", name),
        },
    }
}

async fn execute_read_file(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };

    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            let truncated = if content.len() > MAX_FILE_READ_BYTES {
                format!(
                    "{}\n\n... [TRUNCATED — file is {} bytes, showing first {}]",
                    &content[..MAX_FILE_READ_BYTES],
                    content.len(),
                    MAX_FILE_READ_BYTES
                )
            } else {
                content
            };
            ToolResult {
                success: true,
                output: truncated,
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to read file '{}': {}", path, e),
        },
    }
}

async fn execute_write_file(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'content' argument".to_string(),
            }
        }
    };

    // Ensure parent directory exists
    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult {
                    success: false,
                    output: format!("Failed to create directory '{}': {}", parent.display(), e),
                };
            }
        }
    }

    match tokio::fs::write(path, content).await {
        Ok(_) => ToolResult {
            success: true,
            output: format!("Successfully wrote {} bytes to '{}'", content.len(), path),
        },
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to write file '{}': {}", path, e),
        },
    }
}

fn find_symbol_offsets(
    node: tree_sitter::Node,
    source: &str,
    parts: &[&str],
    current_idx: usize,
) -> Option<(usize, usize)> {
    if current_idx >= parts.len() {
        return None;
    }
    let target = parts[current_idx];

    let is_match = match node.kind() {
        "mod_item" | "struct_item" | "impl_item" | "trait_item" | "function_item" => {
            let mut cursor = node.walk();
            let mut matched = false;
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" || child.kind() == "type_identifier" {
                    if let Ok(name) = child.utf8_text(source.as_bytes()) {
                        if name == target {
                            matched = true;
                            break;
                        }
                    }
                }
            }
            matched
        }
        _ => false,
    };

    if is_match {
        if current_idx == parts.len() - 1 {
            return Some((node.start_byte(), node.end_byte()));
        } else {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(offsets) = find_symbol_offsets(child, source, parts, current_idx + 1) {
                    return Some(offsets);
                }
            }
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(offsets) = find_symbol_offsets(child, source, parts, current_idx) {
                return Some(offsets);
            }
        }
    }
    None
}

async fn execute_edit_file(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };

    let symbol_path = args.get("symbol_path").and_then(|v| v.as_str());
    let old_string = args.get("old_string").and_then(|v| v.as_str());
    let new_string = match args.get("new_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'new_string' argument".to_string(),
            }
        }
    };

    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to read file '{}': {}", path, e),
            }
        }
    };

    let mut replaced_content = String::new();

    if let Some(sym_path) = symbol_path {
        let lang = crate::code_intel::KorgLanguage::from_path(path)
            .unwrap_or(crate::code_intel::KorgLanguage::Rust);
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&lang.tree_sitter_lang()).is_err() {
            return ToolResult {
                success: false,
                output: "Failed to load tree-sitter language parser".to_string(),
            };
        }
        let tree = match parser.parse(&content, None) {
            Some(t) => t,
            None => {
                return ToolResult {
                    success: false,
                    output: "Failed to parse tree-sitter AST".to_string(),
                };
            }
        };

        let parts: Vec<&str> = sym_path.split("::").collect();
        if let Some((start_byte, end_byte)) =
            find_symbol_offsets(tree.root_node(), &content, &parts, 0)
        {
            let mut prefix = content[..start_byte].to_string();
            let suffix = &content[end_byte..];
            prefix.push_str(new_string);
            prefix.push_str(suffix);
            replaced_content = prefix;
        } else {
            return ToolResult {
                success: false,
                output: format!("Symbol path '{}' not found in AST", sym_path),
            };
        }
    } else {
        let old_str =
            match old_string {
                Some(s) => s,
                None => return ToolResult {
                    success: false,
                    output:
                        "Either 'symbol_path' or 'old_string' must be provided for editing a file"
                            .to_string(),
                },
            };

        let count = content.matches(old_str).count();
        if count == 0 {
            return ToolResult {
                success: false,
                output: format!(
                    "old_string not found in '{}'. Make sure it matches exactly (including whitespace and indentation).",
                    path
                ),
            };
        }
        if count > 1 {
            return ToolResult {
                success: false,
                output: format!(
                    "old_string found {} times in '{}'. It must be unique. Add more surrounding context to make it unique.",
                    count, path
                ),
            };
        }
        replaced_content = content.replacen(old_str, new_string, 1);
    }

    let is_rust = path.ends_with(".rs");
    if is_rust {
        let lang = crate::code_intel::KorgLanguage::Rust;
        let content_clone = content.clone();
        let replaced_clone = replaced_content.clone();
        let (pre_anomalies, post_anomalies) = tokio::task::spawn_blocking(move || {
            let pre = crate::code_intel::CodeIntelEngine::validate_syntax(&content_clone, lang);
            let post = crate::code_intel::CodeIntelEngine::validate_syntax(&replaced_clone, lang);
            (pre, post)
        })
        .await
        .unwrap_or_else(|_| (vec![], vec![]));

        if post_anomalies.len() > pre_anomalies.len() {
            let errors: Vec<String> = post_anomalies
                .iter()
                .filter(|a| a.severity == "Error")
                .map(|a| format!("line {}: {}", a.line, a.context))
                .collect();
            if !errors.is_empty() {
                return ToolResult {
                    success: false,
                    output: format!(
                        "Edit rejected: AST degradation detected. Syntax errors introduced:\n{}",
                        errors.join("\n")
                    ),
                };
            }
        }
    }

    let temp_path = format!("{}.tmp", path);
    match tokio::fs::write(&temp_path, &replaced_content).await {
        Ok(_) => {}
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to write to temporary file '{}': {}", temp_path, e),
            };
        }
    }

    if is_rust {
        let _ = tokio::process::Command::new("rustfmt")
            .arg(&temp_path)
            .status()
            .await;

        if let Ok(formatted_content) = tokio::fs::read_to_string(&temp_path).await {
            let final_anomalies = crate::code_intel::CodeIntelEngine::validate_syntax(
                &formatted_content,
                crate::code_intel::KorgLanguage::Rust,
            );
            let has_fatal_error = final_anomalies.iter().any(|a| a.severity == "Error");
            if has_fatal_error {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return ToolResult {
                    success: false,
                    output: "Edit rejected: rustfmt output contains syntax errors.".to_string(),
                };
            }
        }
    }

    let file = match tokio::fs::File::open(&temp_path).await {
        Ok(f) => f,
        Err(e) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return ToolResult {
                success: false,
                output: format!("Failed to open temp file for fsync: {}", e),
            };
        }
    };
    if let Err(e) = file.sync_all().await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return ToolResult {
            success: false,
            output: format!("Failed to fsync temp file: {}", e),
        };
    }
    drop(file);

    if let Err(e) = tokio::fs::rename(&temp_path, path).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return ToolResult {
            success: false,
            output: format!("Failed to atomically rename file to '{}': {}", path, e),
        };
    }

    tracing::info!(
        target: "korg::metrics",
        file = ?path,
        symbol = ?symbol_path,
        "AST structural code edit committed"
    );

    ToolResult {
        success: true,
        output: format!(
            "Successfully edited '{}' using AST structural validation loop.",
            path
        ),
    }
}

#[cfg(unix)]
extern "C" {
    fn setsid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
unsafe fn start_process_group(cmd: &mut tokio::process::Command) {
    use std::os::unix::process::CommandExt;
    cmd.pre_exec(|| {
        unsafe {
            setsid();
        }
        Ok(())
    });
}

#[cfg(not(unix))]
unsafe fn start_process_group(_cmd: &mut tokio::process::Command) {}

#[cfg(unix)]
fn kill_process_group(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        unsafe {
            kill(-(pid as i32), 9); // SIGKILL = 9
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandAuditEvent {
    pub executable: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}

async fn execute_run_command(args: &serde_json::Value) -> ToolResult {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'command' argument".to_string(),
            }
        }
    };
    let working_dir = args
        .get("working_dir")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let config = korg_llm::KorgConfig::load();

    // 1. Path Confinement Check
    let workspace_root = korg_core::paths::project_root();
    let canonical_workspace = match std::fs::canonicalize(&workspace_root) {
        Ok(path) => path,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to canonicalize workspace root: {}", e),
            };
        }
    };

    let canonical_working_dir = match std::fs::canonicalize(working_dir) {
        Ok(path) => path,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!(
                    "Failed to canonicalize working directory '{}': {}",
                    working_dir, e
                ),
            };
        }
    };

    if !canonical_working_dir.starts_with(&canonical_workspace) {
        return ToolResult {
            success: false,
            output: "Access Denied: working directory is outside of the workspace confinement."
                .to_string(),
        };
    }

    // 2. Parsed arguments check & Metacharacter check
    let parsed_args = match shell_words::split(command) {
        Ok(args) => args,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Shell parsing error: {}", e),
            };
        }
    };

    if parsed_args.is_empty() {
        return ToolResult {
            success: false,
            output: "Command is empty".to_string(),
        };
    }

    let executable = &parsed_args[0];
    let argv = &parsed_args[1..];

    // Check policies
    if !config.allow_unsafe_commands && config.sandbox_mode == "strict" {
        let metacharacters = ["&&", "||", "|", ";", "`", "$(", ">", "<", "\n"];
        if metacharacters.iter().any(|&mc| command.contains(mc)) {
            return ToolResult {
                success: false,
                output: format!("Access Denied: command contains disallowed shell metacharacters in strict mode."),
            };
        }
    }

    // Prepare Command
    let mut cmd = if config.allow_unsafe_commands {
        eprintln!(
            "[WARNING] Executing command with implicit shell wrappers: {}",
            command
        );
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    } else {
        let mut c = tokio::process::Command::new(executable);
        c.args(argv);
        c
    };

    cmd.current_dir(&canonical_working_dir);

    // Bounded environment sanitization
    cmd.env_clear();
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }
    if let Ok(cargo_target) = std::env::var("CARGO_TARGET_DIR") {
        cmd.env("CARGO_TARGET_DIR", cargo_target);
    }
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        cmd.env("RUST_LOG", rust_log);
    }
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd.env_remove("OPENAI_API_KEY");
    cmd.env_remove("GROK_API_KEY");

    // Process Group Setup
    unsafe {
        start_process_group(&mut cmd);
    }

    // Configure stdout / stderr pipes
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to spawn command: {}", e),
            };
        }
    };

    let start_time = std::time::Instant::now();
    let timeout_ms = 30_000u64; // Strict 30s timeout ceiling
    let timeout_duration = std::time::Duration::from_millis(timeout_ms);

    use tokio::io::AsyncReadExt;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let stdout_limit = 4 * 1024 * 1024; // 4 MB limit
    let stderr_limit = 4 * 1024 * 1024;

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    let mut stdout_exceeded = false;
    let mut stderr_exceeded = false;

    let mut stdout_done = false;
    let mut stderr_done = false;

    let mut stdout_read_buf = vec![0u8; 8192];
    let mut stderr_read_buf = vec![0u8; 8192];

    let cancellation_token = get_cancellation_token().clone();

    let mut timed_out = false;
    let mut cancelled = false;

    loop {
        let elapsed = start_time.elapsed();
        if elapsed >= timeout_duration {
            timed_out = true;
            break;
        }
        let time_left = timeout_duration - elapsed;

        tokio::select! {
            _ = tokio::time::sleep(time_left) => {
                timed_out = true;
                break;
            }
            _ = cancellation_token.cancelled() => {
                cancelled = true;
                break;
            }
            res = stdout.read(&mut stdout_read_buf), if !stdout_done => {
                match res {
                    Ok(0) => {
                        stdout_done = true;
                    }
                    Ok(n) => {
                        stdout_buf.extend_from_slice(&stdout_read_buf[..n]);
                        if stdout_buf.len() > stdout_limit {
                            stdout_exceeded = true;
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            res = stderr.read(&mut stderr_read_buf), if !stderr_done => {
                match res {
                    Ok(0) => {
                        stderr_done = true;
                    }
                    Ok(n) => {
                        stderr_buf.extend_from_slice(&stderr_read_buf[..n]);
                        if stderr_buf.len() > stderr_limit {
                            stderr_exceeded = true;
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }
        if stdout_done && stderr_done {
            break;
        }
    }

    // Active Process Group Cleanup
    if timed_out || cancelled || stdout_exceeded || stderr_exceeded {
        kill_process_group(&mut child);
    }

    // Wait and reap the child process
    let exit_status = match child.wait().await {
        Ok(s) => s.code().unwrap_or(-1),
        Err(_) => -1,
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;

    let stdout_str = String::from_utf8_lossy(&stdout_buf);
    let stderr_str = String::from_utf8_lossy(&stderr_buf);

    let mut output_str = String::new();
    if stdout_exceeded {
        output_str.push_str(&format!(
            "{}... [TRUNCATED - stdout exceeded 4MB limit]",
            &stdout_str[..stdout_limit.min(stdout_str.len())]
        ));
    } else {
        output_str.push_str(&stdout_str);
    }

    if !stderr_str.is_empty() {
        if !output_str.is_empty() {
            output_str.push_str("\n\n--- STDERR ---\n");
        }
        if stderr_exceeded {
            output_str.push_str(&format!(
                "{}... [TRUNCATED - stderr exceeded 4MB limit]",
                &stderr_str[..stderr_limit.min(stderr_str.len())]
            ));
        } else {
            output_str.push_str(&stderr_str);
        }
    }

    if timed_out {
        output_str = format!(
            "Command timed out after {} ms.\nOutput collected so far:\n{}",
            timeout_ms, output_str
        );
    } else if cancelled {
        output_str = format!(
            "Command cancelled cooperatively.\nOutput collected so far:\n{}",
            output_str
        );
    }

    // Structured Audit Logging
    let audit_event = CommandAuditEvent {
        executable: executable.clone(),
        argv: argv.to_vec(),
        cwd: canonical_working_dir.to_string_lossy().to_string(),
        duration_ms,
        exit_code: exit_status,
        timed_out,
        stdout_bytes: stdout_buf.len(),
        stderr_bytes: stderr_buf.len(),
    };
    tracing::info!(
        target: "korg::execution",
        executable = ?audit_event.executable,
        argv = ?audit_event.argv,
        cwd = ?audit_event.cwd,
        duration_ms = audit_event.duration_ms,
        exit_code = audit_event.exit_code,
        timed_out = audit_event.timed_out,
        stdout_bytes = audit_event.stdout_bytes,
        stderr_bytes = audit_event.stderr_bytes,
        "Command executed"
    );
    eprintln!("[AuditLog] CommandAuditEvent: {:?}", audit_event);

    ToolResult {
        success: !timed_out && !cancelled && exit_status == 0,
        output: output_str,
    }
}

async fn execute_list_directory(args: &serde_json::Value) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut entries = Vec::new();
    if recursive {
        collect_entries_recursive(Path::new(path), &mut entries, 0, CONTEXT_SCAN_DEPTH);
    } else {
        match std::fs::read_dir(path) {
            Ok(dir) => {
                for entry in dir.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let meta = entry.metadata();
                    let suffix = if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                        "/"
                    } else {
                        ""
                    };
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    if suffix == "/" {
                        entries.push(format!("  {}{}", name, suffix));
                    } else {
                        entries.push(format!("  {} ({} bytes)", name, size));
                    }
                }
                entries.sort();
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: format!("Failed to list directory '{}': {}", path, e),
                }
            }
        }
    }

    ToolResult {
        success: true,
        output: entries.join("\n"),
    }
}

fn collect_entries_recursive(
    dir: &Path,
    entries: &mut Vec<String>,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    let indent = "  ".repeat(depth);

    let mut items: Vec<_> = match std::fs::read_dir(dir) {
        Ok(d) => d.flatten().collect(),
        Err(_) => return,
    };
    items.sort_by_key(|e| e.file_name());

    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files and common noise directories
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        let is_dir = entry.metadata().map(|m| m.is_dir()).unwrap_or(false);
        if is_dir {
            entries.push(format!("{}{}/", indent, name));
            collect_entries_recursive(&entry.path(), entries, depth + 1, max_depth);
        } else {
            entries.push(format!("{}{}", indent, name));
        }
    }
}

async fn execute_search_files(args: &serde_json::Value) -> ToolResult {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'pattern' argument".to_string(),
            }
        }
    };
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let include = args.get("include").and_then(|v| v.as_str());

    let mut cmd = tokio::process::Command::new("grep");
    cmd.arg("-rn").arg("--color=never").arg("-I"); // skip binary files

    if let Some(glob) = include {
        cmd.arg("--include").arg(glob);
    }

    cmd.arg(pattern).arg(path);

    match cmd.output().await {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.is_empty() {
                ToolResult {
                    success: true,
                    output: format!("No matches found for '{}'", pattern),
                }
            } else {
                // Truncate to prevent context explosion
                let result = if stdout.len() > 15_000 {
                    let lines: Vec<&str> = stdout.lines().take(100).collect();
                    format!(
                        "{}\n\n... [TRUNCATED — showing first 100 of {} matches]",
                        lines.join("\n"),
                        stdout.lines().count()
                    )
                } else {
                    stdout.to_string()
                };
                ToolResult {
                    success: true,
                    output: result,
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Search failed: {}", e),
        },
    }
}

async fn execute_find_symbols(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };

    let path_buf = std::path::PathBuf::from(path);
    let lang = match crate::code_intel::KorgLanguage::from_path(&path_buf) {
        Some(l) => l,
        None => {
            return ToolResult {
                success: false,
                output: format!(
                    "Unsupported file type for symbol extraction: {:?}",
                    path_buf
                ),
            }
        }
    };

    match tokio::fs::read_to_string(&path_buf).await {
        Ok(content) => {
            let content_clone = content.clone();
            let symbols = tokio::task::spawn_blocking(move || {
                crate::code_intel::CodeIntelEngine::extract_symbols(&content_clone, lang)
            })
            .await
            .unwrap_or_else(|_| vec![]);
            match serde_json::to_string_pretty(&symbols) {
                Ok(json) => ToolResult {
                    success: true,
                    output: json,
                },
                Err(e) => ToolResult {
                    success: false,
                    output: format!("Failed to serialize symbols: {}", e),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to read file '{}': {}", path, e),
        },
    }
}

async fn execute_verify_syntax(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };

    let path_buf = std::path::PathBuf::from(path);
    let lang = match crate::code_intel::KorgLanguage::from_path(&path_buf) {
        Some(l) => l,
        None => {
            return ToolResult {
                success: false,
                output: format!(
                    "Unsupported file type for syntax verification: {:?}",
                    path_buf
                ),
            }
        }
    };

    match tokio::fs::read_to_string(&path_buf).await {
        Ok(content) => {
            let content_clone = content.clone();
            let anomalies = tokio::task::spawn_blocking(move || {
                crate::code_intel::CodeIntelEngine::validate_syntax(&content_clone, lang)
            })
            .await
            .unwrap_or_else(|_| vec![]);
            if anomalies.is_empty() {
                ToolResult {
                    success: true,
                    output: "✅ Pre-flight syntax validation passed. No syntax anomalies or error nodes detected!".to_string(),
                }
            } else {
                match serde_json::to_string_pretty(&anomalies) {
                    Ok(json) => ToolResult {
                        success: true,
                        output: format!(
                            "⚠️ Pre-flight syntax validation detected anomalies:\n{}",
                            json
                        ),
                    },
                    Err(e) => ToolResult {
                        success: false,
                        output: format!("Failed to serialize anomalies: {}", e),
                    },
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to read file '{}': {}", path, e),
        },
    }
}

async fn execute_query_ast(args: &serde_json::Value) -> ToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'path' argument".to_string(),
            }
        }
    };

    let query_str = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'query' argument".to_string(),
            }
        }
    };

    let path_buf = std::path::PathBuf::from(path);
    let lang = match crate::code_intel::KorgLanguage::from_path(&path_buf) {
        Some(l) => l,
        None => {
            return ToolResult {
                success: false,
                output: format!(
                    "Unsupported file type for AST S-expression query: {:?}",
                    path_buf
                ),
            }
        }
    };

    match tokio::fs::read_to_string(&path_buf).await {
        Ok(content) => {
            let content_clone = content.clone();
            let query_str_clone = query_str.to_string();
            let query_res = tokio::task::spawn_blocking(move || {
                crate::code_intel::CodeIntelEngine::query_structure(
                    &content_clone,
                    lang,
                    &query_str_clone,
                )
            })
            .await;
            match query_res {
                Ok(Ok(matches)) => {
                    if matches.is_empty() {
                        ToolResult {
                            success: true,
                            output: format!("No matches found for query '{}'", query_str),
                        }
                    } else {
                        match serde_json::to_string_pretty(&matches) {
                            Ok(json) => ToolResult {
                                success: true,
                                output: json,
                            },
                            Err(e) => ToolResult {
                                success: false,
                                output: format!("Failed to serialize structural matches: {}", e),
                            },
                        }
                    }
                }
                Ok(Err(err_msg)) => ToolResult {
                    success: false,
                    output: format!("Tree-sitter query execution error: {}", err_msg),
                },
                Err(e) => ToolResult {
                    success: false,
                    output: format!("Tree-sitter query task join error: {:?}", e),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to read file '{}': {}", path, e),
        },
    }
}

async fn execute_semantic_search(args: &serde_json::Value) -> ToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return ToolResult {
                success: false,
                output: "Missing 'query' argument".to_string(),
            }
        }
    };
    let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

    let index_path = korg_core::paths::project_root().join(".korg/index.json");
    if !index_path.exists() {
        return ToolResult {
            success: false,
            output: "Codebase semantic index not found. The user must build it first (e.g. using '/index' in shell or via a build step).".to_string(),
        };
    }

    let index = match crate::code_indexer::load_index(&index_path) {
        Ok(idx) => idx,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to load codebase semantic index: {}", e),
            };
        }
    };

    let embedding_model: Box<dyn korg_embeddings::EmbeddingModel> =
        match korg_embeddings::CandleEmbeddingModel::load() {
            Ok(model) => Box::new(model),
            Err(_) => Box::new(korg_embeddings::FakeEmbeddingModel::default()),
        };

    let matches = crate::code_indexer::query_codebase(&index, query, &*embedding_model, top_n);
    if matches.is_empty() {
        return ToolResult {
            success: true,
            output: "No similar code blocks found.".to_string(),
        };
    }

    let mut output = String::new();
    for (i, (sim, block)) in matches.iter().enumerate() {
        output.push_str(&format!(
            "### Result {} (Similarity: {:.4})\nFile: {}\nBlock: {} ({})\nLines: {} - {}\n```{}\n{}\n```\n\n",
            i + 1,
            sim,
            block.file_path,
            block.block_name,
            block.block_type,
            block.start_line,
            block.end_line,
            if block.file_path.ends_with(".rs") { "rust" } else if block.file_path.ends_with(".py") { "python" } else { "" },
            block.content
        ));
    }

    ToolResult {
        success: true,
        output,
    }
}

// =========================================================================
// Agent System Prompt
// =========================================================================

fn build_system_prompt(workspace_root: &str) -> String {
    format!(
        r#"You are Korg, an autonomous software engineering agent. You are working in the directory: {workspace_root}

You can read, write, and edit files, run shell commands, and search the codebase. Your job is to complete the user's task by making real changes to the code.

## Rules
1. Always read relevant files before editing them.
2. Use `edit_file` for targeted changes to existing files. Use `write_file` only for new files or complete rewrites.
3. After making changes, verify them by running the appropriate build/test commands (e.g., `cargo build`, `cargo test`, `npm test`).
4. If a build or test fails, read the error output carefully, fix the issue, and retry.
5. Call `task_complete` when you are done with a summary of all changes.
6. Keep your text responses brief. Focus on tool calls to get work done.
7. Do not ask the user questions — figure it out from the codebase.

## Tool Call Format
When you need to use tools, respond with a JSON array of tool calls:

```json
[{{"tool": "tool_name", "arguments": {{"arg1": "value1"}}}}]
```

You can call multiple tools in a single response. Always respond with EITHER text OR a tool call JSON array, not both."#
    )
}

// =========================================================================
// The Agent Loop
// =========================================================================

/// Result from a full agent run.
#[derive(Debug)]
pub struct AgentRunResult {
    pub summary: String,
    pub turns: usize,
    pub tool_calls_made: usize,
    pub files_modified: Vec<String>,
}

/// Run the full agentic loop: prompt → LLM → tool calls → results → repeat.
pub async fn run_agent_loop(
    prompt: &str,
    provider: Arc<dyn LlmProvider>,
    tui_tx: Option<tokio::sync::mpsc::Sender<crate::tui_bridge::TuiUpdate>>,
) -> Result<AgentRunResult> {
    let workspace_root = korg_core::paths::project_root_string();
    let system_prompt = build_system_prompt(&workspace_root);
    let tools = agent_tool_definitions();

    let mut messages: Vec<Message> = vec![
        Message {
            role: Role::System,
            content: system_prompt,
            name: None,
            tool_calls: None,
        },
        Message {
            role: Role::User,
            content: prompt.to_string(),
            name: None,
            tool_calls: None,
        },
    ];

    let mut total_tool_calls = 0usize;
    let mut files_modified: Vec<String> = Vec::new();
    let mut final_summary = String::new();
    let mut consecutive_text_turns = 0usize;

    let cyan = "\x1b[38;2;0;240;255m";
    let pink = "\x1b[38;2;255;0;180m";
    let green = "\x1b[38;2;0;255;128m";
    let gold = "\x1b[38;2;255;215;0m";
    let slate = "\x1b[38;2;120;125;140m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    for turn in 0..MAX_AGENT_TURNS {
        if get_cancellation_token().is_cancelled() {
            println!(
                "{}⚠ Agent loop aborted due to cooperative cancellation.{}",
                gold, reset
            );
            return Ok(AgentRunResult {
                summary: "Cancelled cooperatively".to_string(),
                turns: turn,
                tool_calls_made: total_tool_calls,
                files_modified,
            });
        }

        // Send to LLM
        let request = LlmRequest {
            messages: messages.clone(),
            temperature: 0.2,
            max_tokens: Some(8192),
            tools: Some(tools.clone()),
            stop_sequences: None,
            multimodal: None,
            tx_id: Some(format!("agent-turn-{}", turn)),
            session_id: None,
            policy_hash: None,
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
            response_format: None,
        };

        println!("\n{slate}──── Agent Turn {} ────{reset}", turn + 1);

        let response = match provider.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}❌ LLM call failed: {}{}", pink, e, reset);
                consecutive_text_turns += 1;
                if consecutive_text_turns >= 3 {
                    eprintln!(
                        "{}⚠ {} consecutive LLM failures. Check your API key and provider config.{}",
                        gold, consecutive_text_turns, reset
                    );
                    final_summary = format!(
                        "Agent stopped: LLM provider '{}' failed {} times consecutively: {}",
                        provider.name(),
                        consecutive_text_turns,
                        e
                    );
                    break;
                }
                // Add error to messages and let the loop try again
                messages.push(Message {
                    role: Role::Assistant,
                    content: format!("LLM call failed: {}", e),
                    name: None,
                    tool_calls: None,
                });
                continue;
            }
        };

        // Try to parse tool calls from the response
        let tool_calls = extract_tool_calls(&response);

        if tool_calls.is_empty() {
            // No tool calls — this is either a text response or completion
            println!("{}🤖 Korg:{} {}", bold, reset, response.content.trim());

            if let Some(ref tx) = tui_tx {
                let _ = tx.try_send(crate::tui_bridge::TuiUpdate::Trace(format!(
                    "🤖 Agent: {}",
                    response.content.chars().take(200).collect::<String>()
                )));
            }

            messages.push(Message {
                role: Role::Assistant,
                content: response.content.clone(),
                name: None,
                tool_calls: None,
            });

            // Check if the LLM thinks it's done
            let lower = response.content.to_lowercase();
            if lower.contains("task_complete")
                || lower.contains("task complete")
                || (lower.contains("done") && lower.contains("summary"))
                || lower.contains("all changes have been")
            {
                final_summary = response.content;
                break;
            }

            consecutive_text_turns += 1;
            if consecutive_text_turns >= 3 {
                // LLM isn't making tool calls — it might not support them
                eprintln!(
                    "{}⚠ Provider returned {} consecutive text responses without tool calls.",
                    "\x1b[38;2;255;215;0m", consecutive_text_turns
                );
                eprintln!(
                    "  Ensure your LLM supports function/tool calling (OpenAI, Anthropic, Grok).{}",
                    reset
                );
                final_summary = response.content;
                break;
            }

            // Prompt it to keep going or use tools
            messages.push(Message {
                role: Role::User,
                content: "Continue working on the task. Use the available tools to make changes. Call task_complete when finished.".to_string(),
                name: None,
                tool_calls: None,
            });
            continue;
        }

        // Execute each tool call
        let mut tool_results = Vec::new();
        consecutive_text_turns = 0; // Reset on tool call

        for tc in &tool_calls {
            total_tool_calls += 1;
            let tool_name = &tc.name;
            let tool_args = &tc.arguments;

            // Track file modifications
            if tool_name == "write_file" || tool_name == "edit_file" {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(tool_args) {
                    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                        if !files_modified.contains(&path.to_string()) {
                            files_modified.push(path.to_string());
                        }
                    }
                }
            }

            // Print tool call info
            let short_args: String = tool_args.chars().take(120).collect();
            println!(
                "  {}⚡ {}{}({}{}{}){}",
                gold, cyan, tool_name, slate, short_args, cyan, reset
            );

            if let Some(ref tx) = tui_tx {
                let _ = tx.try_send(crate::tui_bridge::TuiUpdate::Trace(format!(
                    "⚡ {}({})",
                    tool_name,
                    tool_args.chars().take(80).collect::<String>()
                )));
            }

            // Check for task_complete
            if tool_name == "task_complete" {
                let result = execute_tool(tool_name, tool_args).await;
                final_summary = result.output.clone();
                println!("\n{}✅ Task complete: {}{}", green, final_summary, reset);
                return Ok(AgentRunResult {
                    summary: final_summary,
                    turns: turn + 1,
                    tool_calls_made: total_tool_calls,
                    files_modified,
                });
            }

            let result = execute_tool(tool_name, tool_args).await;

            let status = if result.success {
                format!("{}✓{}", green, reset)
            } else {
                format!("{}✗{}", pink, reset)
            };

            // Show truncated output
            let preview: String = result.output.lines().take(5).collect::<Vec<_>>().join("\n");
            let line_count = result.output.lines().count();
            if line_count > 5 {
                println!(
                    "    {} {}\n    {}... ({} more lines){}",
                    status,
                    preview,
                    slate,
                    line_count - 5,
                    reset
                );
            } else {
                println!("    {} {}", status, preview);
            }

            tool_results.push((tc.clone(), result));
        }

        // Add assistant message with tool calls
        messages.push(Message {
            role: Role::Assistant,
            content: response.content.clone(),
            name: None,
            tool_calls: Some(
                tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| ToolCall {
                        id: format!("call_{}", i),
                        r#type: "function".to_string(),
                        function: korg_llm::FunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        },
                    })
                    .collect(),
            ),
        });

        // Add tool results as messages
        for (i, (tc, result)) in tool_results.iter().enumerate() {
            messages.push(Message {
                role: Role::Tool,
                content: result.output.clone(),
                name: Some(tc.name.clone()),
                tool_calls: None,
            });
        }
    }

    if final_summary.is_empty() {
        final_summary = format!(
            "Agent loop completed after {} turns. {} tool calls made. Files modified: {:?}",
            MAX_AGENT_TURNS, total_tool_calls, files_modified
        );
    }

    Ok(AgentRunResult {
        summary: final_summary,
        turns: MAX_AGENT_TURNS,
        tool_calls_made: total_tool_calls,
        files_modified,
    })
}

// =========================================================================
// Tool Call Extraction
// =========================================================================

/// Internal parsed tool call (before mapping to LLM types)
#[derive(Debug, Clone)]
struct ParsedToolCall {
    name: String,
    arguments: String,
}

/// Extract tool calls from an LLM response.
///
/// Supports multiple formats:
/// 1. Native API tool_calls (OpenAI/Anthropic style)
/// 2. JSON array in the response body: `[{"tool": "name", "arguments": {...}}]`
/// 3. JSON object in the response body: `{"tool": "name", "arguments": {...}}`
fn extract_tool_calls(response: &LlmResponse) -> Vec<ParsedToolCall> {
    // 1. Check native tool_calls from the API
    if let Some(ref tcs) = response.tool_calls {
        if !tcs.is_empty() {
            return tcs
                .iter()
                .map(|tc| ParsedToolCall {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                })
                .collect();
        }
    }

    // 2. Try to parse from the response content
    let content = response.content.trim();

    // Try to find a JSON block in the content
    let json_str = if let Some(start) = content.find("```json") {
        let sub = &content[start + 7..];
        if let Some(end) = sub.find("```") {
            sub[..end].trim()
        } else {
            ""
        }
    } else if content.starts_with('[') || content.starts_with('{') {
        content
    } else if let Some(start) = content.find('[') {
        // Try to find the matching bracket
        let sub = &content[start..];
        if let Some(end) = find_matching_bracket(sub) {
            &sub[..=end]
        } else {
            ""
        }
    } else {
        ""
    };

    if json_str.is_empty() {
        return vec![];
    }

    // Try as array
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
        return arr
            .iter()
            .filter_map(|item| {
                let name = item.get("tool").and_then(|v| v.as_str())?;
                let args = item.get("arguments")?;
                Some(ParsedToolCall {
                    name: name.to_string(),
                    arguments: args.to_string(),
                })
            })
            .collect();
    }

    // Try as single object
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(name) = obj.get("tool").and_then(|v| v.as_str()) {
            if let Some(args) = obj.get("arguments") {
                return vec![ParsedToolCall {
                    name: name.to_string(),
                    arguments: args.to_string(),
                }];
            }
        }
    }

    vec![]
}

/// Find the index of the matching closing bracket for an opening `[` at position 0.
fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in s.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_calls_json_array() {
        let response = LlmResponse {
            content: r#"[{"tool": "read_file", "arguments": {"path": "src/main.rs"}}]"#.to_string(),
            usage: korg_llm::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            model: "test".to_string(),
            finish_reason: korg_llm::FinishReason::Stop,
            tool_calls: None,
        };

        let calls = extract_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_extract_tool_calls_code_block() {
        let response = LlmResponse {
            content: "Let me read the file:\n```json\n[{\"tool\": \"read_file\", \"arguments\": {\"path\": \"Cargo.toml\"}}]\n```".to_string(),
            usage: korg_llm::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            model: "test".to_string(),
            finish_reason: korg_llm::FinishReason::Stop,
            tool_calls: None,
        };

        let calls = extract_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_extract_tool_calls_native() {
        let response = LlmResponse {
            content: String::new(),
            usage: korg_llm::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            model: "test".to_string(),
            finish_reason: korg_llm::FinishReason::ToolCalls,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                r#type: "function".to_string(),
                function: korg_llm::FunctionCall {
                    name: "run_command".to_string(),
                    arguments: r#"{"command": "cargo test"}"#.to_string(),
                },
            }]),
        };

        let calls = extract_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run_command");
    }

    #[test]
    fn test_extract_no_tool_calls() {
        let response = LlmResponse {
            content: "Here is my analysis of the code...".to_string(),
            usage: korg_llm::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            model: "test".to_string(),
            finish_reason: korg_llm::FinishReason::Stop,
            tool_calls: None,
        };

        let calls = extract_tool_calls(&response);
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn test_execute_list_directory() {
        let args = serde_json::json!({"path": ".", "recursive": false});
        let result = execute_list_directory(&args).await;
        assert!(result.success);
        assert!(!result.output.is_empty());
    }

    #[tokio::test]
    async fn test_execute_read_file_missing() {
        let args = serde_json::json!({"path": "/nonexistent/file.rs"});
        let result = execute_read_file(&args).await;
        assert!(!result.success);
        assert!(result.output.contains("Failed to read"));
    }

    #[tokio::test]
    async fn test_execute_semantic_search() {
        let args = serde_json::json!({"query": "agent", "top_n": 2});
        let result = execute_semantic_search(&args).await;

        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let index_exists = manifest_dir.join(".korg/index.json").exists();

        assert_eq!(result.success, index_exists);
        if index_exists {
            assert!(!result.output.is_empty());
        } else {
            assert!(result.output.contains("Codebase semantic index not found"));
        }
    }
}
