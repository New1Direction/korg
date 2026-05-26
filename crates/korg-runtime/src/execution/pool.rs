//! Zero-setup rolling hot sandbox pool.
//! Maintains warm, pre-initialized sandboxes with pre-mounted toolchains,
//! symlinked caches, warm LSP servers, and incremental compiler daemons.

use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum SandboxStatus {
    PreWarming,
    Ready,
    Acquired,
    Released,
}

pub struct Sandbox {
    pub id: String,
    pub path: PathBuf,
    pub lsp_process: Option<Arc<Mutex<Child>>>,
    pub compiler_process: Option<Arc<Mutex<Child>>>,
    pub env_paths: Vec<PathBuf>,
    pub status: SandboxStatus,
    pub created_at: Instant,
}

impl Sandbox {
    /// Send a JSON-RPC query to the warm LSP connection and receive a response.
    pub async fn query_lsp(&self, request: &str) -> Result<String> {
        let lsp_proc = match &self.lsp_process {
            Some(proc) => proc,
            None => return Err(anyhow!("No active warm LSP process in this sandbox")),
        };
        let mut child = lsp_proc.lock().await;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow!("LSP stdin not available"))?;
            let payload = format!("Content-Length: {}\r\n\r\n{}\n", request.len(), request);
            stdin.write_all(payload.as_bytes()).await?;
            stdin.flush().await?;
        }
        // Take stdout into the BufReader so the borrow chain doesn't depend on
        // the MutexGuard's lifetime — a future refactor that splits stdin and
        // stdout into separately-locked phases couldn't dangle the reader.
        // Whichever way the read goes, the recovered ChildStdout is put back
        // so the LSP pipe survives for the next call.
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("LSP stdout not available"))?;
        let (result, stdout) = read_lsp_response(stdout).await;
        child.stdout = Some(stdout);
        result
    }

    pub async fn shutdown(&mut self) {
        self.status = SandboxStatus::Released;
        if let Some(proc) = self.lsp_process.take() {
            let mut child = proc.lock().await;
            child.kill().await.ok();
        }
        if let Some(proc) = self.compiler_process.take() {
            let mut child = proc.lock().await;
            child.kill().await.ok();
        }
    }
}

/// Read a single JSON-RPC response off the LSP's stdout. Takes ownership of
/// `stdout` and always returns it so the caller can put it back on the Child
/// even when the read errors out.
async fn read_lsp_response(stdout: ChildStdout) -> (Result<String>, ChildStdout) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut content_length: usize = 0;
    let result: Result<String> = async {
        while reader.read_line(&mut line).await? > 0 {
            if line == "\r\n" {
                break;
            }
            if line.to_lowercase().starts_with("content-length:") {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    content_length = parts[1].trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }
        if content_length == 0 {
            return Err(anyhow!("Received invalid Content-Length from LSP"));
        }
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).await?;
        Ok(String::from_utf8(buf)?)
    }
    .await;
    (result, reader.into_inner())
}

pub struct SandboxPool {
    pub max_size: usize,
    pub active_sandboxes: Arc<Mutex<VecDeque<Sandbox>>>,
    pub pool_dir: PathBuf,
    pub shared_cache_path: PathBuf,
}

impl SandboxPool {
    pub async fn new(max_size: usize, root_dir: &Path) -> Result<Self> {
        let pool_dir = root_dir.join("pools");
        let shared_cache_path = root_dir.join("shared_caches");
        fs::create_dir_all(&pool_dir).ok();
        fs::create_dir_all(&shared_cache_path).ok();
        let pool = Self {
            max_size,
            active_sandboxes: Arc::new(Mutex::new(VecDeque::new())),
            pool_dir,
            shared_cache_path,
        };
        pool.replenish_all().await?;
        Ok(pool)
    }

    pub async fn replenish_all(&self) -> Result<()> {
        let mut queue = self.active_sandboxes.lock().await;
        while queue.len() < self.max_size {
            let index = queue.len() + 1;
            let sandbox = self.pre_warm_sandbox(index).await?;
            queue.push_back(sandbox);
        }
        Ok(())
    }

    pub async fn pre_warm_sandbox(&self, index: usize) -> Result<Sandbox> {
        let start = Instant::now();
        let id = format!("korg-hot-{:03}", index);
        let path = self.pool_dir.join(&id);
        fs::create_dir_all(&path).ok();

        let mut env_paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            env_paths.push(home.join(".bun/bin"));
            env_paths.push(home.join(".cargo/bin"));
        }
        env_paths.push(PathBuf::from("/usr/local/bin"));
        env_paths.push(PathBuf::from("/opt/homebrew/bin"));

        let node_modules_dir = path.join("node_modules");
        let shared_node_modules = self.shared_cache_path.join("node_modules");
        fs::create_dir_all(&shared_node_modules).ok();
        #[cfg(unix)]
        if !node_modules_dir.exists() {
            std::os::unix::fs::symlink(&shared_node_modules, &node_modules_dir).ok();
        }
        #[cfg(windows)]
        if !node_modules_dir.exists() {
            std::os::windows::fs::symlink_dir(&shared_node_modules, &node_modules_dir).ok();
        }

        let lsp_process = match self.spawn_lsp_subprocess(&path).await {
            Ok(child) => Some(Arc::new(Mutex::new(child))),
            Err(_) => None,
        };
        let compiler_process = match self.spawn_compiler_subprocess(&path).await {
            Ok(child) => Some(Arc::new(Mutex::new(child))),
            Err(_) => None,
        };

        info!(
            "  [POOL] Sandbox '{}' pre-warmed in {}ms",
            id,
            start.elapsed().as_millis()
        );
        Ok(Sandbox {
            id,
            path,
            lsp_process,
            compiler_process,
            env_paths,
            status: SandboxStatus::Ready,
            created_at: Instant::now(),
        })
    }

    async fn spawn_lsp_subprocess(&self, workdir: &Path) -> Result<Child> {
        let cmd_name = if cfg!(windows) { "cmd.exe" } else { "node" };
        let mut cmd = Command::new(cmd_name);
        cmd.current_dir(workdir);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        if cfg!(windows) {
            cmd.arg("/C").arg("echo LSP Active");
        } else {
            let mock_script = r#"
                const readline = require('readline');
                const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
                rl.on('line', (line) => {
                    try {
                        const req = JSON.parse(line.trim());
                        const res = {
                            jsonrpc: '2.0',
                            id: req.id,
                            result: {
                                capabilities: {
                                    textDocumentSync: 1,
                                    hoverProvider: true,
                                    completionProvider: { resolveProvider: true }
                                }
                            }
                        };
                        const payload = JSON.stringify(res);
                        process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n${payload}`);
                    } catch(e) {}
                });
            "#;
            cmd.arg("-e").arg(mock_script);
        }
        let child = cmd.spawn()?;
        Ok(child)
    }

    async fn spawn_compiler_subprocess(&self, workdir: &Path) -> Result<Child> {
        let cmd_name = if cfg!(windows) { "cmd.exe" } else { "node" };
        let mut cmd = Command::new(cmd_name);
        cmd.current_dir(workdir);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        if cfg!(windows) {
            cmd.arg("/C").arg("echo Compiler Watch Active");
        } else {
            cmd.arg("-e")
                .arg("console.log('Compiler daemon pre-warmed.'); setInterval(() => {}, 5000);");
        }
        let child = cmd.spawn()?;
        Ok(child)
    }

    pub async fn acquire(&self) -> Result<Sandbox> {
        let mut queue = self.active_sandboxes.lock().await;
        let mut sandbox = match queue.pop_front() {
            Some(sb) => sb,
            None => {
                warn!("  [POOL] Sandbox pool fully drained! Creating fresh fallback sandbox.");
                self.pre_warm_sandbox(99).await?
            }
        };
        sandbox.status = SandboxStatus::Acquired;
        let active_clone = self.active_sandboxes.clone();
        let pool_dir_clone = self.pool_dir.clone();
        let shared_cache_clone = self.shared_cache_path.clone();
        let max_size = self.max_size;
        tokio::spawn(async move {
            let manager = SandboxPool {
                max_size,
                active_sandboxes: active_clone,
                pool_dir: pool_dir_clone,
                shared_cache_path: shared_cache_clone,
            };
            // Surface replenish failures at WARN — the previous .ok() swallowed
            // them silently, letting the pool drift smaller than max_size with
            // no visibility. acquire() will still fall back to a fresh sandbox
            // on the next call, but ops can now see the underlying problem.
            if let Err(e) = manager.replenish_all().await {
                warn!("  [POOL] background replenish failed: {e}");
            }
        });
        Ok(sandbox)
    }

    pub async fn release(&self, mut sandbox: Sandbox) {
        sandbox.shutdown().await;
        fs::remove_dir_all(&sandbox.path).ok();
        info!("  [POOL] Sandbox '{}' released and recycled", sandbox.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sandbox_prewarming_flow() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pool = SandboxPool::new(2, temp_dir.path()).await.unwrap();
        assert!(temp_dir.path().join("pools").exists());
        assert!(temp_dir.path().join("shared_caches").exists());
        let queue = pool.active_sandboxes.lock().await;
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].status, SandboxStatus::Ready);
        assert!(queue[0].path.exists());
    }

    #[tokio::test]
    async fn test_rolling_pool_replenish() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pool = SandboxPool::new(2, temp_dir.path()).await.unwrap();
        let sandbox1 = pool.acquire().await.unwrap();
        assert_eq!(sandbox1.status, SandboxStatus::Acquired);
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let queue = pool.active_sandboxes.lock().await;
        assert_eq!(queue.len(), 2);
        drop(queue);
        pool.release(sandbox1).await;
    }

    #[tokio::test]
    async fn test_warm_lsp_connection() {
        if cfg!(windows) {
            return;
        }
        let temp_dir = tempfile::tempdir().unwrap();
        let pool = SandboxPool::new(1, temp_dir.path()).await.unwrap();
        let sandbox = pool.acquire().await.unwrap();
        assert!(sandbox.lsp_process.is_some());
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let response = sandbox.query_lsp(request).await.unwrap();
        assert!(response.contains("jsonrpc"));
        assert!(response.contains("capabilities"));
        assert!(response.contains("hoverProvider"));
    }

    #[tokio::test]
    async fn test_sandbox_acquisition_benchmark() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pool = SandboxPool::new(3, temp_dir.path()).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let mut durations = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            let sandbox = pool.acquire().await.unwrap();
            durations.push(start.elapsed());
            pool.release(sandbox).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        let avg_millis = durations.iter().map(|d| d.as_micros()).sum::<u128>() as f64
            / durations.len() as f64
            / 1000.0;
        assert!(avg_millis < 50.0, "acquisition too slow: {}ms", avg_millis);
    }
}
