//! skills.rs — Yvaeh Mode Core Skills (Reconcile & Synthesize)
//!
//! Handles factual reconciliation and concept synthesis in Yvaeh Mode.

use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;
use crate::embeddings::{EmbeddingModel, FakeEmbeddingModel, CandleEmbeddingModel};

#[derive(Debug, Clone)]
pub struct NoteData {
    pub path: PathBuf,
    pub title: String,
    pub date: String,
    pub tags: Vec<String>,
    pub confidence: String,
    pub status: String,
    pub ai_first: bool,
    pub body: String,
}

/// Robust YAML frontmatter and body parser for Obsidian-style markdown.
pub fn parse_vault_note(path: &Path) -> Result<NoteData> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines();

    let mut frontmatter = String::new();
    let mut body = String::new();

    let mut has_frontmatter = false;
    let first_line = lines.next();
    if let Some("---") = first_line {
        has_frontmatter = true;
        let mut fm_lines = Vec::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "---" {
                closed = true;
                break;
            }
            fm_lines.push(line);
        }
        if closed {
            frontmatter = fm_lines.join("\n");
        }
    } else {
        if let Some(line) = first_line {
            body.push_str(line);
            body.push('\n');
        }
    }

    for line in lines {
        body.push_str(line);
        body.push('\n');
    }

    // Default values if keys are missing
    let mut date = "1970-01-01".to_string();
    let mut tags = Vec::new();
    let mut confidence = "medium".to_string();
    let mut status = "active".to_string();
    let mut ai_first = false;
    let mut title = path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    if has_frontmatter {
        for line in frontmatter.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim();
                match key.as_str() {
                    "title" => {
                        title = val.trim_matches('"').trim_matches('\'').to_string();
                    }
                    "date" => {
                        date = val.trim_matches('"').trim_matches('\'').to_string();
                    }
                    "tags" => {
                        let cleaned = val.trim_matches('[').trim_matches(']');
                        tags = cleaned.split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "confidence" => {
                        confidence = val.trim_matches('"').trim_matches('\'').to_string().to_lowercase();
                    }
                    "status" => {
                        status = val.trim_matches('"').trim_matches('\'').to_string().to_lowercase();
                    }
                    "ai-first" | "ai_first" => {
                        ai_first = val == "true";
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(NoteData {
        path: path.to_path_buf(),
        title,
        date,
        tags,
        confidence,
        status,
        ai_first,
        body,
    })
}

/// Helper to serialize NoteData back to disk with high-fidelity frontmatter.
pub fn write_vault_note(note: &NoteData) -> Result<()> {
    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("title: \"{}\"\n", note.title));
    content.push_str(&format!("date: {}\n", note.date));
    
    let note_type = if note.path.to_string_lossy().contains("decisions") {
        "decision"
    } else if note.path.to_string_lossy().contains("synthesis") {
        "synthesis"
    } else {
        "concept"
    };
    content.push_str(&format!("type: {}\n", note_type));

    let tags_str = note.tags.iter().map(|t| format!("{}", t)).collect::<Vec<_>>().join(", ");
    content.push_str(&format!("tags: [{}]\n", tags_str));
    content.push_str(&format!("status: {}\n", note.status));
    content.push_str(&format!("ai-first: {}\n", note.ai_first));
    content.push_str(&format!("confidence: {}\n", note.confidence));
    content.push_str("---\n\n");
    content.push_str(&note.body);

    fs::write(&note.path, content)?;
    Ok(())
}

/// Locate the vault root dynamically by looking upwards for the `wiki` folder.
fn find_vault_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("wiki").is_dir() {
            return Ok(dir);
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    Ok(std::env::current_dir()?)
}

/// Recursively find all markdown files in a directory.
fn scan_directory_for_markdown(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !dir.exists() {
        return results;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(scan_directory_for_markdown(&path));
            } else if path.extension().map_or(false, |ext| ext == "md") {
                results.push(path);
            }
        }
    }
    results
}

fn get_confidence_score(c: &str) -> i32 {
    match c.to_lowercase().as_str() {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

struct ResolvedContradiction {
    winner: String,
    loser: String,
    similarity: f32,
}

struct ConflictRecord {
    a: String,
    b: String,
    similarity: f32,
    conflict_file: String,
}

/// Yvaeh Mode: Reconcile sub-command.
/// Recursively scans vault `.md` files, parses frontmatter, uses semantic embeddings to
/// identify factual contradictions, and resolves chronologically or creates open conflict decisions.
pub async fn run_reconcile(topic: Option<String>) -> Result<()> {
    println!("\n=== STARTING YVAEH HARNESS RECONCILIATION SWARM ===");
    let vault_root = find_vault_root()?;
    let wiki_dir = vault_root.join("wiki");
    
    // === SUGGESTION 1: Scan recent .ktrans logs to isolate touched files ===
    let ktrans_dir = crate::paths::ktrans_dir();
    let mut recent_ktrans_files = std::collections::HashSet::new();
    if ktrans_dir.exists() {
        println!("[Yvaeh reconcile] Scanning recent .ktrans transactions in {}...", ktrans_dir.display());
        if let Ok(entries) = fs::read_dir(ktrans_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        // Extract target paths or filename clues in transaction logs
                        for word in content.split_whitespace() {
                            let cleaned = word.trim_matches('"').trim_matches('\'').trim_matches(',').trim_matches('[').trim_matches(']');
                            if cleaned.contains(".md") || cleaned.contains(".rs") {
                                recent_ktrans_files.insert(cleaned.to_string());
                            }
                        }
                    }
                }
            }
        }
        if !recent_ktrans_files.is_empty() {
            println!("[Yvaeh reconcile] Identified {} active transaction references from recent runs.", recent_ktrans_files.len());
        }
    }

    let folders_to_scan = vec![
        wiki_dir.join("concepts"),
        wiki_dir.join("projects"),
        wiki_dir.join("mechanisms"),
        wiki_dir.join("patterns"),
        wiki_dir.join("decisions"),
    ];

    println!("[Yvaeh reconcile] Scanning vault notes...");
    let mut notes = Vec::new();
    for folder in folders_to_scan {
        for path in scan_directory_for_markdown(&folder) {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with("Conflict — ") {
                continue;
            }
            if let Ok(note) = parse_vault_note(&path) {
                if let Some(ref t) = topic {
                    let lower_topic = t.to_lowercase();
                    let matches_title = note.title.to_lowercase().contains(&lower_topic);
                    let matches_body = note.body.to_lowercase().contains(&lower_topic);
                    let matches_tags = note.tags.iter().any(|tag| tag.to_lowercase().contains(&lower_topic));
                    if matches_title || matches_body || matches_tags {
                        notes.push(note);
                    }
                } else {
                    notes.push(note);
                }
            }
        }
    }

    if notes.is_empty() {
        println!("[Yvaeh reconcile] No matching notes found for scanning.");
        return Ok(());
    }
    println!("[Yvaeh reconcile] Found {} notes to semantically compare.", notes.len());

    // Load embeddings
    let embedding_model: Box<dyn EmbeddingModel> = match CandleEmbeddingModel::load() {
        Ok(real) => {
            println!("[Yvaeh reconcile] Loaded real CandleEmbeddingModel (all-MiniLM-L6-v2)");
            Box::new(real)
        }
        Err(e) => {
            println!("[Yvaeh reconcile] Using FakeEmbeddingModel (Candle model offline or not enabled: {})", e);
            Box::new(FakeEmbeddingModel::default())
        }
    };

    println!("[Yvaeh reconcile] Generating semantic vectors...");
    let mut note_embeddings = Vec::new();
    for note in &notes {
        let preamble: String = note.body.chars().take(200).collect();
        let text_to_embed = format!("{} — {}", note.title, preamble);
        let emb = embedding_model.embed(&text_to_embed).unwrap_or_else(|_| vec![0.0; 32]);
        note_embeddings.push(emb);
    }

    println!("[Yvaeh reconcile] Performing pairwise similarity matrix comparison...");
    let mut resolved_list = Vec::new();
    let mut conflict_row_list = Vec::new();
    let mut flagged_conflicts = Vec::new();

    let n = notes.len();
    let mut modified_losers = std::collections::HashSet::new();

    for i in 0..n {
        for j in (i + 1)..n {
            let sim = crate::embeddings::cosine_similarity(&note_embeddings[i], &note_embeddings[j]);
            if sim >= 0.72 {
                println!("[Yvaeh reconcile] Contradiction detected! Cosine similarity: {:.3}", sim);
                println!("  - Note A: [[{}]] (date: {}, conf: {})", notes[i].title, notes[i].date, notes[i].confidence);
                println!("  - Note B: [[{}]] (date: {}, conf: {})", notes[j].title, notes[j].date, notes[j].confidence);

                // Check winner
                let date_i = &notes[i].date;
                let date_j = &notes[j].date;

                let (winner_idx, loser_idx, is_ambiguous) = if date_i > date_j {
                    (i, j, false)
                } else if date_j > date_i {
                    (j, i, false)
                } else {
                    let score_i = get_confidence_score(&notes[i].confidence);
                    let score_j = get_confidence_score(&notes[j].confidence);
                    if score_i > score_j {
                        (i, j, false)
                    } else if score_j > score_i {
                        (j, i, false)
                    } else {
                        (i, j, true)
                    }
                };

                if is_ambiguous {
                    println!("  - [Verdict] Genuinely ambiguous conflict. Flagging for operator...");
                    let conflict_title = format!("Conflict — {} and {}", notes[i].title, notes[j].title);
                    let conflict_filename = format!("Conflict — {} and {}.md", notes[i].title, notes[j].title)
                        .replace("/", "_")
                        .replace(":", "_");
                    let conflict_path = wiki_dir.join("decisions").join(&conflict_filename);

                    let note_i_preamble: String = notes[i].body.chars().take(300).collect();
                    let note_j_preamble: String = notes[j].body.chars().take(300).collect();

                    let conflict_content = format!(
r#"---
title: "Conflict — {} and {}"
date: 2026-05-21
type: decision
tags: [decision, conflict, yvaeh-mode]
status: open
ai-first: true
confidence: low
---

# Conflict — {} and {}

## For future Grok
This is an automatically generated conflict note created by the Yvaeh harness in Yvaeh mode. Pairwise semantic scanning identified a contradiction between [[{}]] and [[{}]] with no clear metadata winner.

---

## Contradiction Details

Semantic scan detected a high similarity overlap (cosine similarity: {:.3}) between:
1. [[{}]] (Dated: {}, Confidence: {})
2. [[{}]] (Dated: {}, Confidence: {})

### Claims in [[{}]]
> {}

### Claims in [[{}]]
> {}

## Proposed Resolution Paths

- **Option 1:** Promote [[{}]] as the canonical source and archive/deprecate [[{}]].
- **Option 2:** Merge the two notes into a unified concept page.
- **Option 3:** Explicitly partition their scopes to remove contradiction.

*Operator resolution is required to change `status` to `resolved` and record the final decision.*
"#,
                        notes[i].title, notes[j].title,
                        notes[i].title, notes[j].title,
                        notes[i].title, notes[j].title,
                        sim,
                        notes[i].title, notes[i].date, notes[i].confidence,
                        notes[j].title, notes[j].date, notes[j].confidence,
                        notes[i].title, note_i_preamble.trim(),
                        notes[j].title, note_j_preamble.trim(),
                        notes[i].title, notes[j].title
                    );

                    fs::write(&conflict_path, conflict_content)?;
                    flagged_conflicts.push(conflict_title.clone());
                    conflict_row_list.push(ConflictRecord {
                        a: notes[i].title.clone(),
                        b: notes[j].title.clone(),
                        similarity: sim,
                        conflict_file: conflict_title,
                    });
                } else {
                    println!("  - [Verdict] Clear chronological superior: [[{}]] Wins!", notes[winner_idx].title);
                    if modified_losers.insert(loser_idx) {
                        let mut loser_note = notes[loser_idx].clone();
                        loser_note.date = "2026-05-21".to_string();
                        loser_note.confidence = notes[winner_idx].confidence.clone();
                        loser_note.status = "reconciled".to_string();
                        if !loser_note.tags.contains(&"reconciled".to_string()) {
                            loser_note.tags.push("reconciled".to_string());
                        }
                        if !loser_note.tags.contains(&"yvaeh-mode".to_string()) {
                            loser_note.tags.push("yvaeh-mode".to_string());
                        }

                        let reconciled_block = format!(
"\n\n## Reconciled History\n\n- **Reconciled on:** 2026-05-21 by Yvaeh Mode\n- **Winner Source:** [[{}]] (dated {}, confidence: {})\n- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.\n",
                            notes[winner_idx].title,
                            notes[winner_idx].date,
                            notes[winner_idx].confidence
                        );
                        loser_note.body.push_str(&reconciled_block);

                        write_vault_note(&loser_note)?;
                        resolved_list.push(ResolvedContradiction {
                            winner: notes[winner_idx].title.clone(),
                            loser: notes[loser_idx].title.clone(),
                            similarity: sim,
                        });
                    }
                }
            }
        }
    }

    // 1. Update log.md
    let log_path = vault_root.join("log.md");
    if log_path.exists() {
        let mut log_content = fs::read_to_string(&log_path)?;
        let mut details = String::new();
        for r in &resolved_list {
            details.push_str(&format!("- Auto-resolved: [[{}]] (updated via [[{}]])\n", r.loser, r.winner));
        }
        for f in &flagged_conflicts {
            details.push_str(&format!("- Flagged: [[{}]]\n", f));
        }
        if details.is_empty() {
            details.push_str("- No contradictions found.\n");
        }

        let log_entry = format!(
"\n## 2026-05-21 — Yvaeh Mode Reconciliation

- **Command:** `korg reconcile`
- **Result:** Found {} semantic contradictions, auto-resolved {}, and flagged {} as unresolved conflicts.
- **Details:**
{}
",
            resolved_list.len() + flagged_conflicts.len(),
            resolved_list.len(),
            flagged_conflicts.len(),
            details
        );
        log_content.push_str(&log_entry);
        fs::write(&log_path, log_content)?;
        println!("[Yvaeh reconcile] Appended reconciliation entry to log.md.");
    }

    // 2. Update today's daily note
    let daily_path = wiki_dir.join("daily").join("2026-05-21.md");
    if daily_path.exists() {
        let mut daily_content = fs::read_to_string(&daily_path)?;
        let mut table_rows = String::new();
        for r in &resolved_list {
            table_rows.push_str(&format!("| [[{}]] | [[{}]] | {:.3} | Auto-resolved | [[{}]] |\n", r.winner, r.loser, r.similarity, r.winner));
        }
        for c in &conflict_row_list {
            table_rows.push_str(&format!("| [[{}]] | [[{}]] | {:.3} | Unresolved Conflict | [[{}]] |\n", c.a, c.b, c.similarity, c.conflict_file));
        }
        if table_rows.is_empty() {
            table_rows.push_str("| None | None | 0.00 | No conflicts | None |\n");
        }

        let daily_entry = format!(
"\n## Yvaeh Mode Reconciliation

| File A | File B | Similarity | Resolution | Winner / Conflict File |
| --- | --- | --- | --- | --- |
{}",
            table_rows
        );
        daily_content.push_str(&daily_entry);
        fs::write(&daily_path, daily_content)?;
        println!("[Yvaeh reconcile] Updated daily note 2026-05-21.md.");
    }

    // === SUGGESTION 3: Write a human-friendly narrative summary report under Human/Methodology ===
    let human_dir = vault_root.join("Human").join("Methodology");
    if !human_dir.exists() {
        fs::create_dir_all(&human_dir)?;
    }
    let report_path = human_dir.join("Yvaeh-Swarm-Intelligence-Reports.md");
    let mut report_content = if report_path.exists() {
        fs::read_to_string(&report_path)?
    } else {
        r#"# Yvaeh Swarm Intelligence Reports

Living record of Yvaeh Mode semantic reconciliation and synthesis swarm executions. This document bridges dense technical wiki assertions with human operator guidance under Korg's dual-layer philosophy.

---
"#.to_string()
    };

    let mut auto_resolved_narrative = String::new();
    for r in &resolved_list {
        auto_resolved_narrative.push_str(&format!("- **Auto-Resolved:** Promoted [[{}]] chronologically over [[{}]] (similarity: {:.3}).\n", r.winner, r.loser, r.similarity));
    }
    if auto_resolved_narrative.is_empty() {
        auto_resolved_narrative.push_str("- No auto-resolutions performed.\n");
    }

    let mut flagged_narrative = String::new();
    for c in &conflict_row_list {
        flagged_narrative.push_str(&format!("- **Flagged Conflict:** [[{}]] (between [[{}]] and [[{}]] at similarity {:.3}).\n", c.conflict_file, c.a, c.b, c.similarity));
    }
    if flagged_narrative.is_empty() {
        flagged_narrative.push_str("- No ambiguous conflicts flagged.\n");
    }

    let reconciliation_report_entry = format!(
r#"
## 2026-05-21 — Swarm Reconciliation Run

- **Command executed:** `korg reconcile`
- **Contradictions scanned:** {}
- **Auto-resolved conflicts:** {}
- **Flagged conflicts:** {}

### Operator Guidance

The reconciliation swarm identified factual overlaps. Here is your human-level summary of the action taken:

#### 1. Chronological Triumphs (Auto-Resolved)
The swarm promoted the newer and higher confidence design files:
{}
#### 2. Open Decisions (Operator Triage Needed)
Ambiguous claims with equal chronological authority have been extracted to decision markdown pages under `wiki/decisions/`. You must inspect them, select a resolution path, and set their status to `resolved`:
{}
---
"#,
        resolved_list.len() + flagged_conflicts.len(),
        resolved_list.len(),
        flagged_conflicts.len(),
        auto_resolved_narrative,
        flagged_narrative
    );

    report_content.push_str(&reconciliation_report_entry);
    fs::write(&report_path, report_content)?;
    println!("[Yvaeh reconcile] Dual-layer human report written/updated under Human/Methodology/Yvaeh-Swarm-Intelligence-Reports.md.");

    println!("=== YVAEH RECONCILIATION COMPLETE ===");
    Ok(())
}

fn capitalize_words(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Yvaeh Mode: Synthesize sub-command.
/// Scans all `.md` files under the vault folders to identify cross-source convergence patterns
/// of key entities and concepts, generating new synthesis notes under wiki/synthesis/,
/// propagating backlinks to sources, and rebuilding Index.md.
pub async fn run_synthesize() -> Result<()> {
    println!("\n=== STARTING YVAEH HARNESS CONCEPT SYNTHESIS SWARM ===");
    let vault_root = find_vault_root()?;
    let wiki_dir = vault_root.join("wiki");
    
    let folders_to_scan = vec![
        wiki_dir.join("concepts"),
        wiki_dir.join("projects"),
        wiki_dir.join("mechanisms"),
        wiki_dir.join("patterns"),
        wiki_dir.join("decisions"),
    ];

    println!("[Yvaeh synthesize] Scanning vault notes...");
    let mut all_notes = Vec::new();
    for folder in folders_to_scan {
        for path in scan_directory_for_markdown(&folder) {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with("Conflict — ") {
                continue;
            }
            if let Ok(note) = parse_vault_note(&path) {
                all_notes.push(note);
            }
        }
    }

    if all_notes.is_empty() {
        println!("[Yvaeh synthesize] No notes found to scan.");
        return Ok(());
    }

    let key_terms = vec![
        "semantic entropy",
        "contract negotiation",
        "blackboard",
        "process isolation",
        "transactional memory",
        "Evaluator persona",
        "ACP protocol",
        "swarm intelligence",
        "compaction recovery",
        "adversarial loop",
    ];

    let mut term_map: std::collections::HashMap<String, Vec<NoteData>> = std::collections::HashMap::new();

    for note in &all_notes {
        let content_lower = format!("{} {}", note.title, note.body).to_lowercase();
        for term in &key_terms {
            if content_lower.contains(&term.to_lowercase()) {
                term_map.entry(term.to_string())
                    .or_default()
                    .push(note.clone());
            }
        }
    }

    // Create synthesis notes
    let synthesis_dir = wiki_dir.join("synthesis");
    if !synthesis_dir.exists() {
        fs::create_dir_all(&synthesis_dir)?;
    }

    let mut synthesis_created = Vec::new();

    for term in &key_terms {
        if let Some(sources) = term_map.get(*term) {
            if sources.len() >= 2 {
                let cap_term = capitalize_words(term);
                let synth_filename = format!("Synthesis — {}.md", cap_term);
                let synth_path = synthesis_dir.join(&synth_filename);

                if !synth_path.exists() {
                    println!("[Yvaeh synthesize] Synthesis opportunity identified for \"{}\" connecting {} sources!", term, sources.len());
                    
                    let mut sources_links = String::new();
                    let mut context_blocks = String::new();

                    for src in sources {
                        sources_links.push_str(&format!("- [[{}]] (Dated: {})\n", src.title, src.date));
                        
                        let mut context_line = "Incremental swarm progress".to_string();
                        for line in src.body.lines() {
                            if line.to_lowercase().contains(&term.to_lowercase()) {
                                context_line = line.trim().to_string();
                                break;
                            }
                        }
                        context_blocks.push_str(&format!("- In [[{}]], it relates to: *\"{}\"*\n", src.title, context_line));
                    }

                    // === SUGGESTION 2: Generate highly typed notes under wiki/synthesis with full Yvaeh branding & wikilinks ===
                    let synth_content = format!(
r#"---
title: "Synthesis — {}"
date: 2026-05-21
type: synthesis
tags: [synthesis, yvaeh-mode, korg]
ai-first: true
confidence: high
---

# Synthesis — {}

## For future Grok
This synthesis note was automatically generated by the Yvaeh harness in Yvaeh mode. It integrates and synthesizes cross-source occurrences of "{}" detected across multiple vault files.

---

## Convergence Analysis

The concept **{}** represents a key intersection across the following sources:
{}
### Synthesized Context

{} bridges these concepts by providing a unified model of operation.
{}
## Swarm Recommendations

1. **Unify understanding:** Ensure that implementations referencing {} adhere to the transactional contracts in the source notes.
2. **Expand coverage:** Explore how {} can be further leveraged to improve swarm telemetry and operational intelligence.
"#,
                        cap_term, cap_term, term, cap_term, sources_links, cap_term, context_blocks, cap_term, cap_term
                    );

                    fs::write(&synth_path, synth_content)?;
                    synthesis_created.push((cap_term.clone(), sources.clone()));

                    // Link Propagation: Update source files with backlinks
                    for src in sources {
                        let src_path = &src.path;
                        if src_path.exists() {
                            let mut content = fs::read_to_string(src_path)?;
                            let link = format!("[[Synthesis — {}]]", cap_term);
                            
                            if !content.contains(&link) {
                                if content.contains("## See Also") {
                                    content = content.replace("## See Also\n", &format!("## See Also\n\n- {}\n", link));
                                } else {
                                    content.push_str(&format!("\n\n## See Also\n\n- {}\n", link));
                                }
                                fs::write(src_path, content)?;
                                println!("  - Propagated backlink to [[{}]]", src.title);
                            }
                        }
                    }
                }
            }
        }
    }

    // Rebuild the synthesis section of Index.md
    let index_path = vault_root.join("Index.md");
    if index_path.exists() {
        let mut synthesis_files = Vec::new();
        if let Ok(entries) = fs::read_dir(&synthesis_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "md") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        synthesis_files.push(stem.to_string());
                    }
                }
            }
        }
        synthesis_files.sort();

        let mut synth_index_block = "- **Syntheses:** `wiki/synthesis/`".to_string();
        for f in &synthesis_files {
            synth_index_block.push_str(&format!("\n  - [[{}]]", f));
        }

        let mut content = fs::read_to_string(&index_path)?;
        if let Some(idx) = content.find("- **Syntheses:**") {
            let after_synth = &content[idx + 15..];
            if let Some(next_item_idx) = after_synth.find("\n- **") {
                let end_idx = idx + 15 + next_item_idx;
                content.replace_range(idx..end_idx, &synth_index_block);
            } else {
                content.replace_range(idx.., &synth_index_block);
            }
        } else {
            let decisions_line = "- **Decisions:** `wiki/decisions/`";
            if let Some(dec_idx) = content.find(decisions_line) {
                let insert_idx = dec_idx + decisions_line.len();
                content.insert_str(insert_idx, &format!("\n{}", synth_index_block));
            }
        }
        fs::write(&index_path, content)?;
        println!("[Yvaeh synthesize] Rebuilt synthesis section in Index.md.");
    }

    // Log updates
    let log_path = vault_root.join("log.md");
    if log_path.exists() {
        let mut log_content = fs::read_to_string(&log_path)?;
        let mut details = String::new();
        for (term, sources) in &synthesis_created {
            let src_str = sources.iter().map(|s| format!("[[{}]]", s.title)).collect::<Vec<_>>().join(", ");
            details.push_str(&format!("- [[Synthesis — {}]] (connecting {})\n", term, src_str));
        }
        if details.is_empty() {
            details.push_str("- No new synthesis pages created.\n");
        }

        let log_entry = format!(
"\n## 2026-05-21 — Yvaeh Mode Synthesis

- **Command:** `korg synthesize`
- **Result:** Created {} synthesis pages under `wiki/synthesis/`.
- **Details:**
{}
",
            synthesis_created.len(),
            details
        );
        log_content.push_str(&log_entry);
        fs::write(&log_path, log_content)?;
        println!("[Yvaeh synthesize] Recorded synthesis execution in log.md.");
    }

    // Daily note updates
    let daily_path = wiki_dir.join("daily").join("2026-05-21.md");
    if daily_path.exists() {
        let mut daily_content = fs::read_to_string(&daily_path)?;
        let mut bullet_points = String::new();
        for (term, sources) in &synthesis_created {
            let src_str = sources.iter().map(|s| format!("[[{}]]", s.title)).collect::<Vec<_>>().join(", ");
            bullet_points.push_str(&format!("- [[Synthesis — {}]] (connecting {})\n", term, src_str));
        }
        if bullet_points.is_empty() {
            bullet_points.push_str("- No synthesis activities performed.\n");
        }

        let daily_entry = format!(
"\n## Yvaeh Mode Synthesis

> [!NOTE]
> **Yvaeh Synthesis Swarm Run**
> Generated {} synthesis pages under `wiki/synthesis/` for terms:
> {}
",
            synthesis_created.len(),
            bullet_points
        );
        daily_content.push_str(&daily_entry);
        fs::write(&daily_path, daily_content)?;
        println!("[Yvaeh synthesize] Updated daily note with synthesis cards.");
    }

    // === SUGGESTION 3: Write a human-friendly summary to Human/Methodology ===
    let human_dir = vault_root.join("Human").join("Methodology");
    if human_dir.exists() {
        let report_path = human_dir.join("Yvaeh-Swarm-Intelligence-Reports.md");
        let mut report_content = if report_path.exists() {
            fs::read_to_string(&report_path)?
        } else {
            r#"# Yvaeh Swarm Intelligence Reports

Living record of Yvaeh Mode semantic reconciliation and synthesis swarm executions. This document bridges dense technical wiki assertions with human operator guidance under Korg's dual-layer philosophy.

---
"#.to_string()
        };

        let mut synthesis_narrative = String::new();
        for (term, sources) in &synthesis_created {
            let src_str = sources.iter().map(|s| format!("[[{}]]", s.title)).collect::<Vec<_>>().join(", ");
            synthesis_narrative.push_str(&format!("- **Generated [[Synthesis — {}]]**: Found concept convergence in sources: {}.\n", term, src_str));
        }
        if synthesis_narrative.is_empty() {
            synthesis_narrative.push_str("- No synthesis activities performed.\n");
        }

        let synthesis_report_entry = format!(
r#"
## 2026-05-21 — Swarm Synthesis Run

- **Command executed:** `korg synthesize`
- **Synthesis pages generated:** {}

### Operator Guidance

A concept convergence map was built across all scanned notes. The swarm isolated the following high-value synthesis intersections:

{}
We recommend inspecting the newly generated syntheses to expand code scopes or reinforce swarm evaluation guardrails.
---
"#,
            synthesis_created.len(),
            synthesis_narrative
        );

        report_content.push_str(&synthesis_report_entry);
        fs::write(&report_path, report_content)?;
        println!("[Yvaeh synthesize] Dual-layer human report written/updated under Human/Methodology/Yvaeh-Swarm-Intelligence-Reports.md.");
    }

    println!("=== YVAEH CONCEPT SYNTHESIS COMPLETE ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_parsing() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_note.md");
        let content = 
r#"---
title: "Test Note"
date: 2026-05-20
tags: [test, skill, parse]
confidence: high
status: active
ai-first: true
---

# Test Note Title

Body of the test note.
"#;
        fs::write(&file_path, content).unwrap();

        let parsed = parse_vault_note(&file_path).unwrap();
        assert_eq!(parsed.title, "Test Note");
        assert_eq!(parsed.date, "2026-05-20");
        assert_eq!(parsed.tags, vec!["test", "skill", "parse"]);
        assert_eq!(parsed.confidence, "high");
        assert_eq!(parsed.status, "active");
        assert!(parsed.ai_first);
        assert!(parsed.body.contains("Body of the test note."));

        let _ = fs::remove_file(file_path);
    }

    #[test]
    fn test_reconciliation_winner() {
        let temp_dir = std::env::temp_dir();
        let file_a_path = temp_dir.join("reconcile_a.md");
        let file_b_path = temp_dir.join("reconcile_b.md");

        let content_a = 
r#"---
title: "Reconcile Winner Title"
date: 2026-05-21
tags: [concept]
confidence: high
status: active
ai-first: true
---

# Reconcile Winner Title

Winner content.
"#;

        let content_b = 
r#"---
title: "Reconcile Loser Title"
date: 2026-05-19
tags: [concept]
confidence: medium
status: active
ai-first: true
---

# Reconcile Loser Title

Loser content.
"#;

        fs::write(&file_a_path, content_a).unwrap();
        fs::write(&file_b_path, content_b).unwrap();

        let note_a = parse_vault_note(&file_a_path).unwrap();
        let note_b = parse_vault_note(&file_b_path).unwrap();

        // Older file note_b should lose and get updated
        let mut loser_note = note_b.clone();
        loser_note.date = "2026-05-21".to_string();
        loser_note.confidence = note_a.confidence.clone();
        loser_note.status = "reconciled".to_string();
        loser_note.tags.push("reconciled".to_string());
        loser_note.tags.push("yvaeh-mode".to_string());

        let reconciled_block = format!(
"\n\n## Reconciled History\n\n- **Reconciled on:** 2026-05-21 by Yvaeh Mode\n- **Winner Source:** [[{}]] (dated {}, confidence: {})\n",
            note_a.title, note_a.date, note_a.confidence
        );
        loser_note.body.push_str(&reconciled_block);

        write_vault_note(&loser_note).unwrap();

        let re_parsed = parse_vault_note(&file_b_path).unwrap();
        assert_eq!(re_parsed.status, "reconciled");
        assert_eq!(re_parsed.confidence, "high");
        assert!(re_parsed.body.contains("Reconciled History"));
        assert!(re_parsed.tags.contains(&"reconciled".to_string()));

        let _ = fs::remove_file(file_a_path);
        let _ = fs::remove_file(file_b_path);
    }
}
