//! Closed-loop self-healing sandbox recovery engine.
//! Catches command/test failures and executes local repair and validation loops.

use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Intercept a failing command path and perform self-healing.
/// Returns true if the node is successfully repaired.
pub async fn heal_node(
    command: &str,
    logs_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<bool> {
    heal_node_with_context(command, None, None, logs_tx).await
}

/// Dynamic, context-aware self-healing that accepts real compiler error streams and workspace paths.
pub async fn heal_node_with_context(
    command: &str,
    stderr: Option<&str>,
    worktree_path: Option<&Path>,
    logs_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<bool> {
    let start = Instant::now();

    if let Some(ref tx) = logs_tx {
        let _ = tx.send(format!(
            "  [HEAL] Intercepting execution failure from: '{}'",
            command
        ));
    }

    if let (Some(stderr_str), Some(path)) = (stderr, worktree_path) {
        if let Some(ref tx) = logs_tx {
            let _ = tx.send(
                "  [HEAL] [Step 1/4: Diagnosis] Running high-fidelity local error diagnosis..."
                    .to_string(),
            );
        }

        // 1. Missing semicolon
        let re_semicolon = Regex::new(
            r"(?m)error: expected `;`[\s\S]*?-->\s*(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)",
        )
        .unwrap();
        if let Some(caps) = re_semicolon.captures(stderr_str) {
            let file_rel = caps.name("file").unwrap().as_str().trim();
            let line_num: usize = match caps.name("line").unwrap().as_str().parse() {
                Ok(n) => n,
                Err(e) => {
                    if let Some(ref tx) = logs_tx {
                        let _ = tx.send(format!(
                            "  [HEAL] could not parse line number from compiler output: {e}"
                        ));
                    }
                    0
                }
            };
            let file_abs = path.join(file_rel);
            if file_abs.exists() {
                if let Some(ref tx) = logs_tx {
                    let _ = tx.send(format!(
                        "  [HEAL] [Step 2/4: Patching] Diagnosed missing semicolon in {}",
                        file_rel
                    ));
                }
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    if line_num == 0 || line_num > lines.len() {
                        if let Some(ref tx) = logs_tx {
                            let _ = tx.send(format!(
                                "  [HEAL] line {} from compiler output is out of bounds (file has {} lines); skipping",
                                line_num,
                                lines.len()
                            ));
                        }
                    } else if line_num > 0 && line_num <= lines.len() {
                        let line_idx = line_num - 1;
                        let mut new_line = lines[line_idx].clone();
                        new_line.push(';');
                        lines[line_idx] = new_line;
                        let new_content = lines.join("\n") + "\n";
                        if fs::write(&file_abs, new_content).is_ok() {
                            if let Some(ref tx) = logs_tx {
                                let _ = tx.send(format!(
                                    "  [HEAL] Semicolon inserted in {}ms",
                                    start.elapsed().as_millis()
                                ));
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // 2. Unused variable
        let re_unused_var = Regex::new(
            r"(?m)error: unused variable:\s*`(?P<var>[^`]+)`[\s\S]*?-->\s*(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)",
        )
        .unwrap();
        if let Some(caps) = re_unused_var.captures(stderr_str) {
            let file_rel = caps.name("file").unwrap().as_str().trim();
            let line_num: usize = match caps.name("line").unwrap().as_str().parse() {
                Ok(n) => n,
                Err(e) => {
                    if let Some(ref tx) = logs_tx {
                        let _ = tx.send(format!(
                            "  [HEAL] could not parse line number from compiler output: {e}"
                        ));
                    }
                    0
                }
            };
            let var_name = caps.name("var").unwrap().as_str();
            let file_abs = path.join(file_rel);
            if file_abs.exists() {
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    if line_num == 0 || line_num > lines.len() {
                        if let Some(ref tx) = logs_tx {
                            let _ = tx.send(format!(
                                "  [HEAL] line {} from compiler output is out of bounds (file has {} lines); skipping",
                                line_num,
                                lines.len()
                            ));
                        }
                    } else if line_num > 0 && line_num <= lines.len() {
                        let line_idx = line_num - 1;
                        let line = lines[line_idx].clone();
                        let target_var = format!("let {}", var_name);
                        let replacement_var = format!("let _{}", var_name);
                        if line.contains(&target_var) {
                            lines[line_idx] = line.replace(&target_var, &replacement_var);
                            let new_content = lines.join("\n") + "\n";
                            if fs::write(&file_abs, new_content).is_ok() {
                                if let Some(ref tx) = logs_tx {
                                    let _ = tx.send(format!(
                                        "  [HEAL] Prefixed unused variable `{}` with _ in {}ms",
                                        var_name,
                                        start.elapsed().as_millis()
                                    ));
                                }
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }

        // 3. Unused import
        let re_unused_import = Regex::new(
            r"(?m)error: unused import:\s*`(?P<imp>[^`]+)`[\s\S]*?-->\s*(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)",
        )
        .unwrap();
        if let Some(caps) = re_unused_import.captures(stderr_str) {
            let file_rel = caps.name("file").unwrap().as_str().trim();
            let line_num: usize = match caps.name("line").unwrap().as_str().parse() {
                Ok(n) => n,
                Err(e) => {
                    if let Some(ref tx) = logs_tx {
                        let _ = tx.send(format!(
                            "  [HEAL] could not parse line number from compiler output: {e}"
                        ));
                    }
                    0
                }
            };
            let file_abs = path.join(file_rel);
            if file_abs.exists() {
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    if line_num == 0 || line_num > lines.len() {
                        if let Some(ref tx) = logs_tx {
                            let _ = tx.send(format!(
                                "  [HEAL] line {} from compiler output is out of bounds (file has {} lines); skipping",
                                line_num,
                                lines.len()
                            ));
                        }
                    } else if line_num > 0 && line_num <= lines.len() {
                        let line_idx = line_num - 1;
                        let line = lines[line_idx].clone();
                        lines[line_idx] = format!("// {}", line);
                        let new_content = lines.join("\n") + "\n";
                        if fs::write(&file_abs, new_content).is_ok() {
                            if let Some(ref tx) = logs_tx {
                                let _ = tx.send(format!(
                                    "  [HEAL] Commented unused import in {}ms",
                                    start.elapsed().as_millis()
                                ));
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // 4. Missing JS/TS package
        let re_missing_module = Regex::new(
            r"(?i)(?:Cannot find module|Cannot find package|Can't resolve)\s+'(?P<pkg>[@\w\-./]+)'",
        )
        .unwrap();
        if let Some(caps) = re_missing_module.captures(stderr_str) {
            let pkg = caps.name("pkg").unwrap().as_str().trim();
            let base_pkg = if pkg.starts_with('@') {
                let parts: Vec<&str> = pkg.split('/').collect();
                if parts.len() >= 2 {
                    format!("{}/{}", parts[0], parts[1])
                } else {
                    pkg.to_string()
                }
            } else {
                pkg.split('/').next().unwrap_or(pkg).to_string()
            };
            let mut cmd = tokio::process::Command::new("bun");
            cmd.arg("add").arg(&base_pkg).current_dir(path);
            if let Ok(output) = cmd.output().await {
                if output.status.success() {
                    if let Some(ref tx) = logs_tx {
                        let _ = tx.send(format!(
                            "  [HEAL] Auto-installed '{}' in {}ms",
                            base_pkg,
                            start.elapsed().as_millis()
                        ));
                    }
                    return Ok(true);
                }
            }
        }

        // 5. Const reassignment (JS/TS)
        let re_node_const = Regex::new(r"(?i)Assignment to constant variable").unwrap();
        let re_at_file =
            Regex::new(r"(?m)at\s+(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)").unwrap();
        let re_ts_const = Regex::new(
            r"(?m)^(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)\s+-\s+error TS2588: Cannot assign to '(?P<var>[^']+)'",
        )
        .unwrap();
        let mut matched_const = false;
        let mut file_rel_const = "";
        let mut line_num_const: usize = 0;
        let mut var_name_const = String::new();
        if re_node_const.is_match(stderr_str) {
            if let Some(caps) = re_at_file.captures(stderr_str) {
                file_rel_const = caps.name("file").unwrap().as_str().trim();
                line_num_const = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
                matched_const = true;
            }
        } else if let Some(caps) = re_ts_const.captures(stderr_str) {
            file_rel_const = caps.name("file").unwrap().as_str().trim();
            line_num_const = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
            var_name_const = caps.name("var").unwrap().as_str().to_string();
            matched_const = true;
        }
        if matched_const && !file_rel_const.is_empty() {
            let file_abs = path.join(file_rel_const);
            if file_abs.exists() {
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    let start_search = std::cmp::min(line_num_const, lines.len());
                    let mut healed = false;
                    if !var_name_const.is_empty() {
                        let target = format!("const {}", var_name_const);
                        for i in (0..start_search).rev() {
                            if lines[i].contains(&target) {
                                lines[i] =
                                    lines[i].replace(&target, &format!("let {}", var_name_const));
                                healed = true;
                                break;
                            }
                        }
                    } else {
                        for i in (0..start_search).rev() {
                            if lines[i].contains("const ") {
                                lines[i] = lines[i].replace("const ", "let ");
                                healed = true;
                                break;
                            }
                        }
                    }
                    if healed {
                        let new_content = lines.join("\n") + "\n";
                        if fs::write(&file_abs, new_content).is_ok() {
                            if let Some(ref tx) = logs_tx {
                                let _ = tx.send(format!(
                                    "  [HEAL] Converted const to let in {}ms",
                                    start.elapsed().as_millis()
                                ));
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // 6. TS7006 implicit any parameter
        let re_ts_any_err = Regex::new(r"(?m)(?:error TS7006: Parameter '(?P<param1>[^']+)' implicitly has an 'any' type.*-->\s*(?P<file1>[^\n:]+):(?P<line1>\d+):(?P<col1>\d+)|(?P<file2>[^\n:]+)\((?P<line2>\d+),(?P<col2>\d+)\): error TS7006: Parameter '(?P<param2>[^']+)' implicitly has an 'any' type)").unwrap();
        if let Some(caps) = re_ts_any_err.captures(stderr_str) {
            let file_rel = caps
                .name("file1")
                .or_else(|| caps.name("file2"))
                .unwrap()
                .as_str()
                .trim();
            let line_num: usize = caps
                .name("line1")
                .or_else(|| caps.name("line2"))
                .unwrap()
                .as_str()
                .parse()
                .unwrap_or(0);
            let param_name = caps
                .name("param1")
                .or_else(|| caps.name("param2"))
                .unwrap()
                .as_str();
            let file_abs = path.join(file_rel);
            if file_abs.exists() {
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    if line_num == 0 || line_num > lines.len() {
                        if let Some(ref tx) = logs_tx {
                            let _ = tx.send(format!(
                                "  [HEAL] line {} from compiler output is out of bounds (file has {} lines); skipping",
                                line_num,
                                lines.len()
                            ));
                        }
                    } else if line_num > 0 && line_num <= lines.len() {
                        let line_idx = line_num - 1;
                        let line = lines[line_idx].clone();
                        let re_param =
                            Regex::new(&format!(r"\b{}\b", regex::escape(param_name))).unwrap();
                        if re_param.is_match(&line) {
                            lines[line_idx] = re_param
                                .replace(&line, &format!("{}: any", param_name))
                                .into_owned();
                            let new_content = lines.join("\n") + "\n";
                            if fs::write(&file_abs, new_content).is_ok() {
                                if let Some(ref tx) = logs_tx {
                                    let _ = tx.send(format!(
                                        "  [HEAL] Annotated `{}` with ': any' in {}ms",
                                        param_name,
                                        start.elapsed().as_millis()
                                    ));
                                }
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }

        // 7. TS6133/6192/6196 unused local/import suppression
        let re_paren = Regex::new(r"(?m)^(?P<file>[^\n:]+)\((?P<line>\d+),(?P<col>\d+)\):\s+error TS(?P<code>6133|6192|6196)").unwrap();
        let re_colon_dash = Regex::new(r"(?m)^(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)\s+-\s+error TS(?P<code>6133|6192|6196)").unwrap();
        let re_colon = Regex::new(r"(?m)^(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+):\s+error TS(?P<code>6133|6192|6196)").unwrap();
        let re_multiline = Regex::new(r"(?m)error TS(?P<code>6133|6192|6196):[\s\S]*?-->\s*(?P<file>[^\n:]+):(?P<line>\d+):(?P<col>\d+)").unwrap();
        let mut matched_ts = false;
        let mut file_rel_ts = "";
        let mut line_num_ts: usize = 0;
        if let Some(caps) = re_paren.captures(stderr_str) {
            file_rel_ts = caps.name("file").unwrap().as_str().trim();
            line_num_ts = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
            matched_ts = true;
        } else if let Some(caps) = re_colon_dash.captures(stderr_str) {
            file_rel_ts = caps.name("file").unwrap().as_str().trim();
            line_num_ts = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
            matched_ts = true;
        } else if let Some(caps) = re_colon.captures(stderr_str) {
            file_rel_ts = caps.name("file").unwrap().as_str().trim();
            line_num_ts = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
            matched_ts = true;
        } else if let Some(caps) = re_multiline.captures(stderr_str) {
            file_rel_ts = caps.name("file").unwrap().as_str().trim();
            line_num_ts = caps.name("line").unwrap().as_str().parse().unwrap_or(0);
            matched_ts = true;
        }
        if matched_ts && !file_rel_ts.is_empty() {
            let file_abs = path.join(file_rel_ts);
            if file_abs.exists() {
                if let Ok(content) = fs::read_to_string(&file_abs) {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    if line_num_ts > 0 && line_num_ts <= lines.len() {
                        let line_idx = line_num_ts - 1;
                        let line = lines[line_idx].clone();
                        let leading = line
                            .chars()
                            .take_while(|c| c.is_whitespace())
                            .collect::<String>();
                        lines.insert(line_idx, format!("{}// @ts-ignore", leading));
                        let new_content = lines.join("\n") + "\n";
                        if fs::write(&file_abs, new_content).is_ok() {
                            if let Some(ref tx) = logs_tx {
                                let _ = tx.send(format!(
                                    "  [HEAL] Added @ts-ignore suppression in {}ms",
                                    start.elapsed().as_millis()
                                ));
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // 8. Rust unresolved crate (E0432)
        let re_rust_crate_err =
            Regex::new(r"(?m)error\[E0432\]: unresolved import `(?P<crate>[^`:]+)(?:::.*)?`")
                .unwrap();
        if let Some(caps) = re_rust_crate_err.captures(stderr_str) {
            let crate_name = caps.name("crate").unwrap().as_str().trim();
            if crate_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                let mut cmd = tokio::process::Command::new("cargo");
                cmd.arg("add").arg(crate_name).current_dir(path);
                if let Ok(output) = cmd.output().await {
                    if output.status.success() {
                        if let Some(ref tx) = logs_tx {
                            let _ = tx.send(format!(
                                "  [HEAL] Auto-added crate '{}' in {}ms",
                                crate_name,
                                start.elapsed().as_millis()
                            ));
                        }
                        return Ok(true);
                    }
                }
            }
        }
    }

    // No compiler stderr + worktree path, or no known pattern matched: there is
    // nothing real to repair here. Report honestly that we did not heal — the
    // caller decides what to do with a genuine failure. Never fake success.
    if let Some(ref tx) = logs_tx {
        let _ = tx.send("  [HEAL] No actionable error context — cannot auto-heal".to_string());
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_recovery_flow() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        // Without compiler stderr + a worktree path there is nothing real to
        // repair, so heal_node must report that honestly — no fake "healed".
        let healed = heal_node("thump test --fail", Some(tx)).await.unwrap();
        assert!(!healed, "heal_node with no error/worktree context cannot heal");
        let mut logs = Vec::new();
        while let Ok(log) = rx.try_recv() {
            logs.push(log);
        }
        assert!(logs.iter().any(|l| l.contains("[HEAL] Intercepting")));
    }

    #[tokio::test]
    async fn test_semicolon_self_healing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {\n    let x = 42\n}").unwrap();
        let compiler_stderr = "error: expected `;`, found `}`\n  --> src/main.rs:2:15";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let healed = heal_node_with_context(
            "cargo check",
            Some(compiler_stderr),
            Some(dir.path()),
            Some(tx),
        )
        .await
        .unwrap();
        assert!(healed);
        let corrected = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(corrected, "fn main() {\n    let x = 42;\n}\n");
    }

    #[tokio::test]
    async fn test_unused_var_self_healing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {\n    let x = 42;\n}").unwrap();
        let compiler_stderr = "error: unused variable: `x`\n  --> src/main.rs:2:9";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let healed = heal_node_with_context(
            "cargo check",
            Some(compiler_stderr),
            Some(dir.path()),
            Some(tx),
        )
        .await
        .unwrap();
        assert!(healed);
        let corrected = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(corrected, "fn main() {\n    let _x = 42;\n}\n");
    }

    #[tokio::test]
    async fn test_const_reassignment_healing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.ts");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "const x = 42;\nx = 43;\n").unwrap();
        let compiler_stderr = "TypeError: Assignment to constant variable.\n  at src/main.ts:2:1";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let healed = heal_node_with_context(
            "bun run src/main.ts",
            Some(compiler_stderr),
            Some(dir.path()),
            Some(tx),
        )
        .await
        .unwrap();
        assert!(healed);
        let corrected = std::fs::read_to_string(&file_path).unwrap();
        assert!(corrected.contains("let x = 42;"));
    }

    #[tokio::test]
    async fn test_ts_implicit_any_healing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.ts");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(
            &file_path,
            "function greet(name) {\n  console.log(name);\n}",
        )
        .unwrap();
        let compiler_stderr =
            "src/main.ts(1,16): error TS7006: Parameter 'name' implicitly has an 'any' type.";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let healed =
            heal_node_with_context("tsc", Some(compiler_stderr), Some(dir.path()), Some(tx))
                .await
                .unwrap();
        assert!(healed);
        let corrected = std::fs::read_to_string(&file_path).unwrap();
        assert!(corrected.contains("function greet(name: any)"));
    }

    #[tokio::test]
    async fn test_ts_unused_var_healing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.ts");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(
            &file_path,
            "import { foo } from './bar';\nconsole.log('hello');",
        )
        .unwrap();
        let compiler_stderr =
            "src/main.ts(1,10): error TS6192: All imports in import declaration are unused.";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let healed =
            heal_node_with_context("tsc", Some(compiler_stderr), Some(dir.path()), Some(tx))
                .await
                .unwrap();
        assert!(healed);
        let corrected = std::fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = corrected.lines().collect();
        assert!(lines[0].contains("@ts-ignore"));
        assert!(lines[1].contains("import { foo }"));
    }

    #[tokio::test]
    async fn test_healing_pipeline_benchmark() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.ts");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let compiler_stderr =
            "src/main.ts(1,16): error TS7006: Parameter 'name' implicitly has an 'any' type.";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut durations = Vec::new();
        for i in 0..10 {
            std::fs::write(
                &file_path,
                format!("function greet(name) {{ console.log(name, {}); }}", i),
            )
            .unwrap();
            let start_t = Instant::now();
            let healed = heal_node_with_context(
                "tsc",
                Some(compiler_stderr),
                Some(dir.path()),
                Some(tx.clone()),
            )
            .await
            .unwrap();
            durations.push(start_t.elapsed());
            assert!(healed);
        }
        let avg_millis = durations.iter().map(|d| d.as_micros()).sum::<u128>() as f64
            / durations.len() as f64
            / 1000.0;
        assert!(avg_millis < 100.0, "healing too slow: {}ms", avg_millis);
    }
}
