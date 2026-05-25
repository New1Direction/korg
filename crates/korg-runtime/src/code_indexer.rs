//! Semantic Codebase Context Indexer
//!
//! Recursively crawls files in a workspace directory, splits them structurally
//! into logical codeblocks, generates dense vector embeddings, and supports
//! fast cosine similarity matches.

use anyhow::{Context, Result};
use korg_embeddings::EmbeddingModel;
use korg_embeddings::{cosine_similarity, CodebaseIndex, IndexedCodeBlock};
use std::fs;
use std::path::{Path, PathBuf};

/// Check if a path should be ignored (e.g., build folders, dependency caches, media files)
pub fn should_ignore(path: &Path) -> bool {
    let name = match path.file_name().and_then(|s| s.to_str()) {
        Some(n) => n,
        None => return false,
    };

    // Ignore list
    name.starts_with('.')
        || name == "target"
        || name == "node_modules"
        || name == "build"
        || name == "dist"
        || name == "Cargo.lock"
        || name == "package-lock.json"
        || name == "venv"
        || name == "env"
}

/// Recursively gather all indexable source code files in a directory
pub fn recurse_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if should_ignore(dir) {
        return Ok(());
    }

    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                recurse_files(&path, files)?;
            } else {
                let indexable = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| {
                        ext == "rs"
                            || ext == "toml"
                            || ext == "js"
                            || ext == "ts"
                            || ext == "py"
                            || ext == "md"
                    })
                    .unwrap_or(false);

                if indexable && !should_ignore(&path) {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

/// Splits a source file into functional code blocks based on structural keywords
pub fn split_file(path: &Path, root_dir: &str) -> std::io::Result<Vec<IndexedCodeBlock>> {
    let content = fs::read_to_string(path)?;
    let relative_path = path
        .strip_prefix(root_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(vec![]);
    }

    let mut blocks = vec![];

    if ext == "rs" {
        // Rust structural splitting
        let mut current_block_start = 0;
        let mut current_block_name = "module".to_string();
        let mut current_block_type = "module".to_string();

        for (i, line) in lines.iter().enumerate() {
            let line_trimmed = line.trim();

            // Check structural boundary keywords
            let is_boundary = line_trimmed.starts_with("pub struct ")
                || line_trimmed.starts_with("struct ")
                || line_trimmed.starts_with("pub impl ")
                || line_trimmed.starts_with("impl ")
                || line_trimmed.starts_with("pub fn ")
                || line_trimmed.starts_with("fn ")
                || line_trimmed.starts_with("pub enum ")
                || line_trimmed.starts_with("enum ")
                || line_trimmed.starts_with("pub mod ")
                || line_trimmed.starts_with("mod ")
                || line_trimmed.starts_with("pub trait ")
                || line_trimmed.starts_with("trait ")
                || line_trimmed.starts_with("#[cfg(test)]")
                || line_trimmed.starts_with("mod tests");

            if is_boundary && i > current_block_start {
                // Save previous block
                let block_content = lines[current_block_start..i].join("\n");
                if !block_content.trim().is_empty() {
                    blocks.push(IndexedCodeBlock {
                        file_path: relative_path.clone(),
                        block_name: current_block_name.clone(),
                        block_type: current_block_type.clone(),
                        start_line: current_block_start + 1,
                        end_line: i,
                        content: block_content,
                        embedding: vec![],
                    });
                }

                // Determine name and type of new block
                current_block_start = i;
                current_block_type = if line_trimmed.contains("struct") {
                    "struct".to_string()
                } else if line_trimmed.contains("impl") {
                    "impl".to_string()
                } else if line_trimmed.contains("fn") {
                    "fn".to_string()
                } else if line_trimmed.contains("enum") {
                    "enum".to_string()
                } else if line_trimmed.contains("mod") {
                    "module".to_string()
                } else if line_trimmed.contains("trait") {
                    "trait".to_string()
                } else {
                    "generic".to_string()
                };

                // Extract name
                current_block_name = line_trimmed
                    .split_whitespace()
                    .nth(if line_trimmed.starts_with("pub") {
                        2
                    } else {
                        1
                    })
                    .map(|s| s.trim_end_matches('{').trim_end_matches('(').to_string())
                    .unwrap_or_else(|| "block".to_string());
            }
        }

        // Save last block
        let block_content = lines[current_block_start..].join("\n");
        if !block_content.trim().is_empty() {
            blocks.push(IndexedCodeBlock {
                file_path: relative_path.clone(),
                block_name: current_block_name,
                block_type: current_block_type,
                start_line: current_block_start + 1,
                end_line: lines.len(),
                content: block_content,
                embedding: vec![],
            });
        }
    } else if ext == "md" {
        // Markdown header splitting
        let mut current_block_start = 0;
        let mut current_block_name = "introduction".to_string();

        for (i, line) in lines.iter().enumerate() {
            if line.starts_with('#') {
                if i > current_block_start {
                    let block_content = lines[current_block_start..i].join("\n");
                    if !block_content.trim().is_empty() {
                        blocks.push(IndexedCodeBlock {
                            file_path: relative_path.clone(),
                            block_name: current_block_name.clone(),
                            block_type: "section".to_string(),
                            start_line: current_block_start + 1,
                            end_line: i,
                            content: block_content,
                            embedding: vec![],
                        });
                    }
                }
                current_block_start = i;
                current_block_name = line.trim_start_matches('#').trim().to_string();
            }
        }

        let block_content = lines[current_block_start..].join("\n");
        if !block_content.trim().is_empty() {
            blocks.push(IndexedCodeBlock {
                file_path: relative_path,
                block_name: current_block_name,
                block_type: "section".to_string(),
                start_line: current_block_start + 1,
                end_line: lines.len(),
                content: block_content,
                embedding: vec![],
            });
        }
    } else {
        // Chunk splitting for other files (toml, js, python)
        let chunk_size = 40;
        for chunk_idx in 0..=(lines.len() / chunk_size) {
            let start = chunk_idx * chunk_size;
            let end = std::cmp::min(start + chunk_size, lines.len());
            if start < end {
                let block_content = lines[start..end].join("\n");
                if !block_content.trim().is_empty() {
                    blocks.push(IndexedCodeBlock {
                        file_path: relative_path.clone(),
                        block_name: format!("chunk-{}", chunk_idx),
                        block_type: "chunk".to_string(),
                        start_line: start + 1,
                        end_line: end,
                        content: block_content,
                        embedding: vec![],
                    });
                }
            }
        }
    }

    Ok(blocks)
}

/// Recursively indexes the entire workspace using the provided embedding model
pub async fn index_workspace(root: &str, model: &dyn EmbeddingModel) -> Result<CodebaseIndex> {
    let root_path = Path::new(root);
    let mut files = vec![];
    recurse_files(root_path, &mut files).context("Failed to recurse files in workspace")?;

    let mut codebase_index = CodebaseIndex::default();

    for file in files {
        if let Ok(mut blocks) = split_file(&file, root) {
            for block in &mut blocks {
                // Generate dense semantic embedding vector
                if let Ok(emb) = model.embed(&block.content) {
                    block.embedding = emb;
                    codebase_index.blocks.push(block.clone());
                }
            }
        }
    }

    Ok(codebase_index)
}

/// Query the codebase using semantic vector similarity search
pub fn query_codebase(
    index: &CodebaseIndex,
    query: &str,
    model: &dyn EmbeddingModel,
    top_n: usize,
) -> Vec<(f32, IndexedCodeBlock)> {
    let query_emb = match model.embed(query) {
        Ok(emb) => emb,
        Err(_) => return vec![],
    };

    let mut matches = vec![];

    for block in &index.blocks {
        let sim = cosine_similarity(&query_emb, &block.embedding);
        matches.push((sim, block.clone()));
    }

    // Sort descending by cosine similarity score
    matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    matches.into_iter().take(top_n).collect()
}

/// Persist semantic index data to a local file
pub fn save_index(index: &CodebaseIndex, path: &str) -> Result<()> {
    let parent = Path::new(path).parent();
    if let Some(p) = parent {
        let _ = fs::create_dir_all(p);
    }
    let serialized =
        serde_json::to_string_pretty(index).context("Failed to serialize codebase index")?;
    fs::write(path, serialized).context("Failed to write index to disk")?;
    Ok(())
}

/// Load a previously saved codebase semantic index
pub fn load_index<P: AsRef<Path>>(path: P) -> Result<CodebaseIndex> {
    let content = fs::read_to_string(path).context("Failed to read codebase index file")?;
    let index =
        serde_json::from_str(&content).context("Failed to deserialize codebase index JSON")?;
    Ok(index)
}
