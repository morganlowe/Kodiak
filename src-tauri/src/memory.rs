//! Local Memory Bank (`~/.local/share/kodiak/memory/*.md`) (§1 Slice 2.1).
//! Plain Markdown files with YAML frontmatter.

use crate::config::{load_or_seed_config, memory_dir_path, GrantLevel};
use crate::enforcer::{log_decision, ActionClass};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Distinct newtype for memory excerpts sent to cloud stages (§1.2 Slice 2.4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryExcerpt(pub String);

impl MemoryExcerpt {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for MemoryExcerpt {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for MemoryExcerpt {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MemoryExcerpt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Distinct newtype for full memory bodies delivered to local execution stages (§1.2 Slice 2.4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBody(pub String);

impl MemoryBody {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::ops::Deref for MemoryBody {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for MemoryBody {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MemoryBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MemoryBody {
    fn from(s: String) -> Self {
        MemoryBody(s)
    }
}

impl From<&str> for MemoryBody {
    fn from(s: &str) -> Self {
        MemoryBody(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// File stem/slug (e.g., "frank-base-specs" from "frank-base-specs.md")
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// "global" or "project:<path>"
    pub scope: String,
    /// Whether full text may be sent to cloud stages (cloud_share: yes/no)
    pub cloud_share: bool,
    pub updated: String,
    /// Raw markdown body (excluding YAML frontmatter)
    pub body: MemoryBody,
    /// True if frontmatter was missing/corrupt and default values were applied
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCatalogItem {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub scope: String,
    pub cloud_share: bool,
    pub excerpt: MemoryExcerpt,
}

impl MemoryEntry {
    pub fn excerpt(&self) -> MemoryExcerpt {
        let mut out = String::new();
        for line in self.body.as_str().lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(trimmed);
            if out.len() >= 200 {
                break;
            }
        }
        if out.is_empty() {
            return MemoryExcerpt("(empty memory body)".to_string());
        }
        if out.len() > 200 {
            MemoryExcerpt(format!("{}...", &out[..197]))
        } else {
            MemoryExcerpt(out)
        }
    }

    pub fn to_catalog_item(&self) -> MemoryCatalogItem {
        MemoryCatalogItem {
            id: self.id.clone(),
            title: self.title.clone(),
            tags: self.tags.clone(),
            scope: self.scope.clone(),
            cloud_share: self.cloud_share,
            excerpt: self.excerpt(),
        }
    }
}

/// Parses raw memory markdown content with optional YAML frontmatter `--- ... ---`.
pub fn parse_memory_content(id: &str, raw: &str) -> Result<MemoryEntry, String> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(MemoryEntry {
            id: id.to_string(),
            title: id.to_string(),
            tags: Vec::new(),
            scope: "global".to_string(),
            cloud_share: false,
            updated: Utc::now().to_rfc3339(),
            body: MemoryBody(raw.to_string()),
            warning: Some("Missing YAML frontmatter (defaulted to global, cloud_share=no)".to_string()),
        });
    }

    // Look for closing ---
    let after_start = &trimmed[3..];
    if let Some(close_idx) = after_start.find("\n---") {
        let frontmatter_str = &after_start[..close_idx];
        let body = after_start[close_idx + 4..].trim_start().to_string();

        let mut title = id.to_string();
        let mut tags: Vec<String> = Vec::new();
        let mut scope = "global".to_string();
        let mut cloud_share = false;
        let mut updated = Utc::now().to_rfc3339();
        let mut warning = None;

        for line in frontmatter_str.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = l.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim();
                match key.as_str() {
                    "title" => {
                        title = val.trim_matches('"').trim_matches('\'').to_string();
                    }
                    "tags" => {
                        let inner = val.trim_matches('[').trim_matches(']');
                        tags = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "scope" => {
                        scope = val.trim_matches('"').trim_matches('\'').to_string();
                    }
                    "cloud_share" => {
                        let clean = val.trim_matches('"').trim_matches('\'').to_lowercase();
                        cloud_share = matches!(clean.as_str(), "yes" | "true" | "1");
                    }
                    "updated" => {
                        updated = val.trim_matches('"').trim_matches('\'').to_string();
                    }
                    _ => {}
                }
            }
        }

        if title.is_empty() {
            title = id.to_string();
            warning = Some("Missing title in frontmatter".to_string());
        }

        Ok(MemoryEntry {
            id: id.to_string(),
            title,
            tags,
            scope,
            cloud_share,
            updated,
            body: MemoryBody(body),
            warning,
        })
    } else {
        Ok(MemoryEntry {
            id: id.to_string(),
            title: id.to_string(),
            tags: Vec::new(),
            scope: "global".to_string(),
            cloud_share: false,
            updated: Utc::now().to_rfc3339(),
            body: MemoryBody(raw.to_string()),
            warning: Some("Unclosed YAML frontmatter (defaulted to global, cloud_share=no)".to_string()),
        })
    }
}

/// Parses a memory markdown file with optional YAML frontmatter `--- ... ---`.
pub fn parse_memory_file(path: &Path) -> Result<MemoryEntry, String> {
    let id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read memory file {}: {e}", path.display()))?;

    parse_memory_content(&id, &raw)
}

pub fn serialize_memory_file(entry: &MemoryEntry) -> String {
    let cloud_share_str = if entry.cloud_share { "yes" } else { "no" };
    let tags_str = entry
        .tags
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "---\ntitle: \"{}\"\ntags: [{}]\nscope: \"{}\"\ncloud_share: {}\nupdated: \"{}\"\n---\n\n{}",
        entry.title.replace('"', "\\\""),
        tags_str,
        entry.scope.replace('"', "\\\""),
        cloud_share_str,
        entry.updated,
        entry.body.as_str().trim_start()
    )
}

pub fn load_all_memories() -> Result<Vec<MemoryEntry>, String> {
    let dir = memory_dir_path()?;
    let entries = fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read memory directory {}: {e}", dir.display()))?;

    let mut list = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && p.extension().map_or(false, |ext| ext == "md") {
            if let Ok(mem) = parse_memory_file(&p) {
                list.push(mem);
            }
        }
    }
    list.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(list)
}

/// Strips ANSI escape sequences (e.g. CSI codes `\x1b[...m`) from a string (§5 Slice 2.4).
pub fn strip_ansi_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&p) = chars.peek() {
                    if (p >= '0' && p <= '9') || p == ';' || p == '?' || (p >= ' ' && p <= '/') {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(&f) = chars.peek() {
                    if f >= '@' && f <= '~' {
                        chars.next();
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next(); // consume ']'
                while let Some(o) = chars.next() {
                    if o == '\x07' || o == '\x1b' {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Determines if a string is symbol-heavy (>60% of non-whitespace characters are non-alphanumeric).
pub fn is_symbol_heavy(s: &str) -> bool {
    let non_ws: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    if non_ws.is_empty() {
        return false;
    }
    let non_alnum = non_ws.iter().filter(|c| !c.is_alphanumeric()).count();
    (non_alnum as f64 / non_ws.len() as f64) > 0.60
}

/// Extracts the right-hand text column if the line has a symbol-heavy ASCII art prefix
/// separated by 3 or more spaces from an alphanumeric label/spec (§5 Slice 2.4).
fn extract_twocolumn_text(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut last_gap_start = None;
    let mut last_gap_end = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if i - start >= 3 {
                last_gap_start = Some(start);
                last_gap_end = Some(i);
            }
        } else {
            i += 1;
        }
    }

    if let (Some(gap_start), Some(gap_end)) = (last_gap_start, last_gap_end) {
        let left = line[..gap_start].trim();
        let right = line[gap_end..].trim();
        if !left.is_empty()
            && !right.is_empty()
            && is_symbol_heavy(left)
            && !is_symbol_heavy(right)
            && right.len() >= 2
        {
            return Some(right);
        }
    }
    None
}

/// Result of memory body sanitation (§5 Slice 2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanitizeOutcome {
    Unchanged,
    Cleaned { old_chars: usize, new_chars: usize },
    SkippedExcessiveRemoval { old_chars: usize, attempted_chars: usize },
}

/// Sanitizes memory body content using deterministic local heuristics (§5 Slice 2.4).
///
/// 1. Strips ANSI escape sequences.
/// 2. Strips contiguous ASCII-art/box-drawing lines and prefixes (>60% non-alphanumeric).
/// 3. Collapses 3+ blank lines to one.
/// 4. Trims trailing whitespace.
/// 5. Hard guarantee: If cleaning would remove >50% of body non-whitespace content, skips rewrite.
pub fn sanitize_memory_body(raw_body: &str) -> (String, SanitizeOutcome) {
    let without_ansi = strip_ansi_escapes(raw_body);
    let mut processed_lines: Vec<String> = Vec::new();

    for line in without_ansi.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            processed_lines.push(String::new());
            continue;
        }

        if let Some(right_text) = extract_twocolumn_text(trimmed_end) {
            processed_lines.push(right_text.to_string());
        } else if is_symbol_heavy(trimmed_end) {
            // Drop pure ASCII art line
            continue;
        } else {
            processed_lines.push(trimmed_end.to_string());
        }
    }

    // Collapse 3+ blank lines to one (at most 1 empty line between content lines)
    let mut collapsed_lines: Vec<String> = Vec::new();
    let mut consecutive_blanks = 0;
    for l in processed_lines {
        if l.is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 1 {
                collapsed_lines.push(l);
            }
        } else {
            consecutive_blanks = 0;
            collapsed_lines.push(l);
        }
    }

    let mut candidate = collapsed_lines.join("\n");
    if raw_body.ends_with('\n') && !candidate.ends_with('\n') {
        candidate.push('\n');
    }

    let old_chars = raw_body.len();
    let new_chars = candidate.len();

    // Check removal ratio on non-whitespace content
    let old_non_ws = raw_body.chars().filter(|c| !c.is_whitespace()).count();
    let new_non_ws = candidate.chars().filter(|c| !c.is_whitespace()).count();
    let removal_ratio = if old_non_ws > 0 {
        old_non_ws.saturating_sub(new_non_ws) as f64 / old_non_ws as f64
    } else {
        0.0
    };

    if removal_ratio > 0.50 {
        (
            raw_body.to_string(),
            SanitizeOutcome::SkippedExcessiveRemoval {
                old_chars,
                attempted_chars: new_chars,
            },
        )
    } else if candidate == raw_body {
        (raw_body.to_string(), SanitizeOutcome::Unchanged)
    } else {
        (candidate, SanitizeOutcome::Cleaned { old_chars, new_chars })
    }
}

pub fn save_memory_entry(entry: &MemoryEntry) -> Result<MemoryEntry, String> {
    let auto_sanitize = load_or_seed_config().map(|c| c.memory_auto_sanitize).unwrap_or(true);
    save_memory_entry_with_options(entry, auto_sanitize)
}

pub fn save_memory_entry_with_options(
    entry: &MemoryEntry,
    auto_sanitize: bool,
) -> Result<MemoryEntry, String> {
    let dir = memory_dir_path()?;
    let clean_id = entry
        .id
        .trim()
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");
    let slug = if clean_id.is_empty() {
        "unnamed-memory".to_string()
    } else {
        clean_id
    };

    let path = dir.join(format!("{slug}.md"));
    let orig_path = dir.join(format!("{slug}.md.orig"));

    let mut updated_entry = entry.clone();
    updated_entry.id = slug.clone();

    if auto_sanitize {
        let (cleaned_body, outcome) = sanitize_memory_body(entry.body.as_str());
        match outcome {
            SanitizeOutcome::Cleaned { old_chars, new_chars } => {
                // Preserve original bytes before rewrite as rolling .orig backup (§5 Slice 2.4)
                if path.exists() {
                    let _ = fs::copy(&path, &orig_path);
                } else {
                    let original_full = serialize_memory_file(entry);
                    let _ = fs::write(&orig_path, original_full);
                }

                log_decision(
                    "memory_management",
                    0,
                    ActionClass::Create,
                    &path.to_string_lossy(),
                    "memory_cleaned",
                    &dir.to_string_lossy(),
                    GrantLevel::Read,
                    false,
                    false,
                    "memory_cleaned",
                    &format!("memory_cleaned: {slug}, {old_chars} → {new_chars} chars, backup kept"),
                );

                updated_entry.body = MemoryBody(cleaned_body);
                updated_entry.updated = Utc::now().to_rfc3339();
                let content = serialize_memory_file(&updated_entry);
                fs::write(&path, content)
                    .map_err(|e| format!("Failed to write memory file {}: {e}", path.display()))?;
                return Ok(updated_entry);
            }
            SanitizeOutcome::SkippedExcessiveRemoval { old_chars, attempted_chars } => {
                log_decision(
                    "memory_management",
                    0,
                    ActionClass::Create,
                    &path.to_string_lossy(),
                    "memory_clean_skipped",
                    &dir.to_string_lossy(),
                    GrantLevel::Read,
                    false,
                    false,
                    "memory_clean_skipped",
                    &format!(
                        "memory_clean_skipped: {slug}, removal would exceed 50% ({old_chars} → {attempted_chars} chars)"
                    ),
                );
            }
            SanitizeOutcome::Unchanged => {
                // If nothing to remove: no rewrite, no note, mtime untouched
                if path.exists() {
                    if let Ok(existing) = fs::read_to_string(&path) {
                        let candidate_full = serialize_memory_file(entry);
                        if existing == candidate_full {
                            return Ok(entry.clone());
                        }
                    }
                }
            }
        }
    }

    updated_entry.updated = Utc::now().to_rfc3339();
    let content = serialize_memory_file(&updated_entry);
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write memory file {}: {e}", path.display()))?;

    Ok(updated_entry)
}

pub fn delete_memory_entry(id: &str) -> Result<PathBuf, String> {
    let dir = memory_dir_path()?;
    let path = dir.join(format!("{id}.md"));
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete memory file {}: {e}", path.display()))?;
        Ok(path)
    } else {
        Err(format!("Memory file {} not found", path.display()))
    }
}

/// Matches memories relevant to prompt and active project.
/// Also extracts any explicit `@memory-slug` or `@title` references in the prompt.
pub fn match_memories(
    memories: &[MemoryEntry],
    prompt: &str,
    project_dir: &Path,
) -> (Vec<MemoryEntry>, Vec<String>) {
    let prompt_lower = prompt.to_lowercase();
    let prompt_words: Vec<&str> = prompt_lower
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|w| w.len() >= 3)
        .collect();

    let project_str = project_dir.to_string_lossy().to_lowercase();

    let mut matched = Vec::new();
    let mut explicit_refs = Vec::new();

    for mem in memories {
        // Check scope applicability:
        // if scope is project:<path>, it only matches if path aligns with project_dir
        if let Some(scoped_path) = mem.scope.strip_prefix("project:") {
            let norm_scoped = scoped_path.trim().to_lowercase();
            if !project_str.contains(&norm_scoped) && !norm_scoped.contains(&project_str) {
                continue;
            }
        }

        let at_ref = format!("@{}", mem.id.to_lowercase());
        let at_title_ref = format!("@{}", mem.title.to_lowercase().replace(' ', "-"));
        let has_explicit = prompt_lower.contains(&at_ref) || prompt_lower.contains(&at_title_ref);

        if has_explicit {
            explicit_refs.push(mem.id.clone());
            matched.push(mem.clone());
            continue;
        }

        // Tag or keyword matching
        let mut score = 0;
        for tag in &mem.tags {
            let t_low = tag.to_lowercase();
            if prompt_words.contains(&t_low.as_str()) || prompt_lower.contains(&t_low) {
                score += 2;
            }
            if project_str.contains(&t_low) {
                score += 1;
            }
        }

        let id_words: Vec<&str> = mem.id.split('-').collect();
        for w in id_words {
            if prompt_words.contains(&w) {
                score += 1;
            }
        }

        if score > 0 {
            matched.push(mem.clone());
        }
    }

    (matched, explicit_refs)
}

/// Builds the memory catalog string for cloud prompts (capped ~500 tokens).
pub fn build_cloud_memory_catalog(memories: &[MemoryEntry]) -> String {
    if memories.is_empty() {
        return "No memories recorded.".to_string();
    }
    let mut out = String::new();
    for mem in memories {
        let cat = mem.to_catalog_item();
        let tags_str = if cat.tags.is_empty() {
            String::new()
        } else {
            format!(" [tags: {}]", cat.tags.join(", "))
        };
        let share_str = if cat.cloud_share {
            "cloud_share=yes"
        } else {
            "cloud_share=no (local-only)"
        };
        out.push_str(&format!(
            "- @{} \"{}\" ({}) {}: {}\n",
            cat.id, cat.title, share_str, tags_str, cat.excerpt.as_str()
        ));
    }
    if out.len() > 2500 {
        out.truncate(2500);
        out.push_str("\n... [catalog truncated to cap cloud tokens]");
    }
    out
}

/// Builds the full local context for Ollama stages (draft_code, etc.).
pub fn build_local_memory_context(memories: &[MemoryEntry]) -> MemoryBody {
    if memories.is_empty() {
        return MemoryBody(String::new());
    }
    let mut out = String::from("Memory Bank Entries (Local Knowledge):\n");
    for mem in memories {
        out.push_str(&format!(
            "\n--- Memory: {} (@{}) ---\n{}\n",
            mem.title, mem.id, mem.body.as_str()
        ));
    }
    MemoryBody(out)
}

/// Resolves memories referenced by a plan step (via `step.memories_used`, `@name` or `@@name` citations,
/// or title/instruction keywords). Returns matching memory entries and a combined payload bounded
/// by `budget_chars`.
pub fn resolve_step_memories(
    all_memories: &[MemoryEntry],
    step_memories_used: &[String],
    step_title: &str,
    step_instruction: &str,
    plan_memories_used: &[String],
    budget_chars: usize,
) -> (Vec<MemoryEntry>, MemoryBody) {
    let mut matched: Vec<MemoryEntry> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    let title_lower = step_title.to_lowercase();
    let inst_lower = step_instruction.to_lowercase();

    // 1. Direct step_memories_used
    for item in step_memories_used {
        let clean = item
            .trim()
            .trim_start_matches('@')
            .trim_start_matches('@')
            .to_lowercase();
        if let Some(mem) = all_memories.iter().find(|m| {
            m.id.to_lowercase() == clean || m.title.to_lowercase() == clean
        }) {
            if seen_ids.insert(mem.id.clone()) {
                matched.push(mem.clone());
            }
        }
    }

    // 2. Scan title and instruction for @slug, @@slug, or exact memory id / title
    for mem in all_memories {
        if seen_ids.contains(&mem.id) {
            continue;
        }
        let at_ref1 = format!("@{}", mem.id.to_lowercase());
        let at_ref2 = format!("@@{}", mem.id.to_lowercase());
        let at_title1 = format!("@{}", mem.title.to_lowercase().replace(' ', "-"));
        let at_title2 = format!("@@{}", mem.title.to_lowercase().replace(' ', "-"));

        if title_lower.contains(&at_ref1)
            || title_lower.contains(&at_ref2)
            || title_lower.contains(&at_title1)
            || title_lower.contains(&at_title2)
            || inst_lower.contains(&at_ref1)
            || inst_lower.contains(&at_ref2)
            || inst_lower.contains(&at_title1)
            || inst_lower.contains(&at_title2)
        {
            seen_ids.insert(mem.id.clone());
            matched.push(mem.clone());
        }
    }

    // 3. Fallback to plan_memories_used:
    // If step_memories_used was empty, check if plan_memories_used has entries.
    // If the step mentions "memory", OR matches any memory tag/id word,
    // OR if the plan has memories and this is a drafting/comparing/reading step, deliver it!
    for item in plan_memories_used {
        let clean = item
            .trim()
            .trim_start_matches('@')
            .trim_start_matches('@')
            .to_lowercase();
        if let Some(mem) = all_memories.iter().find(|m| {
            m.id.to_lowercase() == clean || m.title.to_lowercase() == clean
        }) {
            if seen_ids.contains(&mem.id) {
                continue;
            }
            // Check if step mentions the memory or general subject
            let id_words: Vec<&str> = mem.id.split('-').collect();
            let matches_subject = title_lower.contains("memory")
                || inst_lower.contains("memory")
                || title_lower.contains(&mem.id.to_lowercase())
                || inst_lower.contains(&mem.id.to_lowercase())
                || id_words.iter().any(|w| w.len() >= 4 && (title_lower.contains(w) || inst_lower.contains(w)))
                || mem.tags.iter().any(|t| {
                    let t_low = t.to_lowercase();
                    t_low.len() >= 4 && (title_lower.contains(&t_low) || inst_lower.contains(&t_low))
                });

            // Also if plan explicitly declared this memory and the step is drafting or comparing,
            // deliver to ensure the local worker has authoritative ground truth
            let is_drafting_or_comparing = title_lower.contains("draft")
                || title_lower.contains("compar")
                || title_lower.contains("summar")
                || title_lower.contains("spec")
                || inst_lower.contains("draft")
                || inst_lower.contains("compar")
                || inst_lower.contains("summar")
                || inst_lower.contains("spec");

            if matches_subject || is_drafting_or_comparing {
                seen_ids.insert(mem.id.clone());
                matched.push(mem.clone());
            }
        }
    }

    // 4. Keyword fallback: if title or instruction matches memory tags/title words
    if matched.is_empty() {
        for mem in all_memories {
            let id_clean = mem.id.replace('-', " ");
            if title_lower.contains(&id_clean) || inst_lower.contains(&id_clean) {
                if seen_ids.insert(mem.id.clone()) {
                    matched.push(mem.clone());
                }
            }
        }
    }

    // Build bounded payload
    if matched.is_empty() {
        return (matched, MemoryBody(String::new()));
    }

    let mut payload = String::new();
    let mut truncated = false;
    for mem in &matched {
        let block = format!(
            "--- Referenced Memory: {} (@{}) ---\n{}\n",
            mem.title, mem.id, mem.body.as_str().trim()
        );
        if payload.len() + block.len() > budget_chars {
            let remain = budget_chars.saturating_sub(payload.len());
            if remain > 0 {
                let slice = &block[..block.len().min(remain)];
                payload.push_str(slice);
            }
            truncated = true;
            break;
        } else {
            payload.push_str(&block);
        }
    }

    if truncated {
        payload.push_str(&format!(
            "\n[Note: Memory delivery truncated at {budget_chars} chars per memory_step_budget_chars]\n"
        ));
    }

    (matched, MemoryBody(payload))
}


#[cfg(test)]
mod memory_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_memory_crud_frontmatter_fallback_and_matching() {
        let tmp = std::env::temp_dir().join("foreman_memory_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // 1. File with valid frontmatter
        let f1 = tmp.join("frank-base-specs.md");
        fs::write(
            &f1,
            "---\ntitle: Frank base specs\ntags: [hardware, frank, specs]\nscope: global\ncloud_share: no\nupdated: \"2026-11-07T13:00:00Z\"\n---\n\nHost: Frank, Ryzen AI 9 HX 370, RTX 4050 6GB VRAM, Arch Linux."
        ).unwrap();

        let mem1 = parse_memory_file(&f1).unwrap();
        assert_eq!(mem1.id, "frank-base-specs");
        assert_eq!(mem1.title, "Frank base specs");
        assert_eq!(mem1.tags, vec!["hardware", "frank", "specs"]);
        assert_eq!(mem1.scope, "global");
        assert!(!mem1.cloud_share);
        assert!(mem1.warning.is_none());
        assert!(mem1.excerpt().contains("Host: Frank"));

        // 2. Corrupt/missing frontmatter file
        let f2 = tmp.join("random-note.md");
        fs::write(&f2, "# Just a random note\nWithout frontmatter header").unwrap();
        let mem2 = parse_memory_file(&f2).unwrap();
        assert_eq!(mem2.id, "random-note");
        assert_eq!(mem2.scope, "global");
        assert!(!mem2.cloud_share);
        assert!(mem2.warning.is_some());

        // 3. Project scoped file
        let f3 = tmp.join("ai-test-context.md");
        fs::write(
            &f3,
            "---\ntitle: Ai-test specifics\ntags: [testing]\nscope: project:/tmp/test-project/Ai-test\ncloud_share: yes\nupdated: \"2026-11-07T13:00:00Z\"\n---\n\nProject specifics here."
        ).unwrap();
        let mem3 = parse_memory_file(&f3).unwrap();
        assert!(mem3.cloud_share);

        let all = vec![mem1, mem2, mem3];

        // 4. Test matching: prompt mentions @frank-base-specs
        let (matched_explicit, explicit_ids) = match_memories(&all, "compare @frank-base-specs with modern laptop", Path::new("/tmp"));
        assert_eq!(explicit_ids, vec!["frank-base-specs"]);
        assert!(matched_explicit.iter().any(|m| m.id == "frank-base-specs"));

        // 5. Test matching: project scope filtering
        let (matched_proj, _) = match_memories(&all, "do something testing", Path::new("/tmp/test-project/Ai-test"));
        assert!(matched_proj.iter().any(|m| m.id == "ai-test-context"));

        let (matched_other, _) = match_memories(&all, "do something testing", Path::new("/tmp/test-project/Other"));
        assert!(!matched_other.iter().any(|m| m.id == "ai-test-context"));

        // 6. Test cloud catalog capping and content
        let catalog = build_cloud_memory_catalog(&all);
        assert!(catalog.contains("@frank-base-specs"));
        assert!(catalog.contains("cloud_share=no (local-only)"));
        assert!(catalog.contains("cloud_share=yes"));
    }

    #[test]
    fn test_sanitizer_strips_art_keeps_specs() {
        let raw = "\
Host machine specifications for Frank:
- CPU: AMD Ryzen AI 9 HX 370 (12 cores, 24 threads)
- GPU: NVIDIA GeForce RTX 4050 Laptop GPU (6 GB VRAM)
- RAM: 32 GB LPDDR5x
- OS: Arch Linux / CachyOS Linux x86_64
\x1b[32m- Storage: 1TB NVMe\x1b[0m
Details:




           .-------------------------:                    OS: CachyOS x86_64
          .+=========================.                    Host: ProArt PX13
         :++===++==================-       :++-           Kernel: Linux 7.2.10
        :*++====+++++=============-        .==:           CPU: AMD Ryzen AI 9 HX 370
       -*+++=====+***++==========:                        GPU: NVIDIA GeForce RTX 4050
";
        let (cleaned, outcome) = sanitize_memory_body(raw);
        assert!(matches!(outcome, SanitizeOutcome::Cleaned { .. }));

        // Every spec line survives
        assert!(cleaned.contains("Host machine specifications for Frank:"));
        assert!(cleaned.contains("- CPU: AMD Ryzen AI 9 HX 370"));
        assert!(cleaned.contains("- GPU: NVIDIA GeForce RTX 4050"));
        assert!(cleaned.contains("- RAM: 32 GB LPDDR5x"));
        assert!(cleaned.contains("- Storage: 1TB NVMe"));
        assert!(cleaned.contains("OS: CachyOS x86_64"));
        assert!(cleaned.contains("Host: ProArt PX13"));
        assert!(cleaned.contains("Kernel: Linux 7.2.10"));
        assert!(cleaned.contains("CPU: AMD Ryzen AI 9 HX 370"));
        assert!(cleaned.contains("GPU: NVIDIA GeForce RTX 4050"));

        // ANSI escape codes removed
        assert!(!cleaned.contains("\x1b[32m"));
        assert!(!cleaned.contains("\x1b[0m"));

        // Pure art blocks removed
        assert!(!cleaned.contains(".-------------------------:"));
        assert!(!cleaned.contains(".+=========================."));
        assert!(!cleaned.contains(":++===++==================-"));

        // Excess blank lines collapsed
        assert!(!cleaned.contains("\n\n\n"));
    }

    #[test]
    fn test_sanitizer_never_touches_frontmatter() {
        let file_content = "\
---
title: \"Frank base specs\"
tags: [\"hardware\", \"frank\", \"specs\"]
scope: \"global\"
cloud_share: yes
updated: \"2026-11-07T14:47:16Z\"
---

Host machine specs:
- OS: CachyOS Linux x86_64
- Host: ASUS ProArt PX13
- CPU: AMD Ryzen AI 9 HX 370 (24) @ 5.16 GHz
- GPU: NVIDIA GeForce RTX 4050 Max-Q / Mobile
- Memory: 32 GB LPDDR5X @ 7500 MT/s
- Disk: 1 TB NVMe SSD
\x1b[31m+------------------------+\x1b[0m
|  box drawing art block |
+------------------------+
";
        let parsed = parse_memory_content("frank-specs", file_content).unwrap();
        let (cleaned_body, outcome) = sanitize_memory_body(parsed.body.as_str());
        assert!(matches!(outcome, SanitizeOutcome::Cleaned { .. }));

        let mut updated = parsed.clone();
        updated.body = MemoryBody(cleaned_body);
        updated.updated = parsed.updated.clone(); // preserve timestamp to verify frontmatter verbatim
        let serialized = serialize_memory_file(&updated);

        let orig_fm = file_content.split("---").nth(1).unwrap().trim();
        let new_fm = serialized.split("---").nth(1).unwrap().trim();
        assert_eq!(orig_fm, new_fm, "YAML front-matter must remain 100% verbatim");
    }

    #[test]
    fn test_sanitizer_backup_and_skip_behavior() {
        use uuid::Uuid;
        let tmp = std::env::temp_dir().join(format!("foreman-sanitize-test-{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&tmp);

        // Case A: Excessive removal (>50% body non-whitespace is art) -> SKIPPED
        let excessive_art = "\
Host:
.================================================.
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
:++++++++++++++++++++++++++++++++++++++++++++++++:
";
        let (body_a, outcome_a) = sanitize_memory_body(excessive_art);
        assert!(matches!(outcome_a, SanitizeOutcome::SkippedExcessiveRemoval { .. }));
        assert_eq!(body_a, excessive_art, "Excessive removal must leave body verbatim");

        // Case B: Moderate removal -> backup .orig created on save
        let moderate = MemoryEntry {
            id: "moderate-memory".to_string(),
            title: "Moderate Memory".to_string(),
            tags: vec!["test".to_string()],
            scope: "global".to_string(),
            cloud_share: false,
            updated: "2026-11-07T13:00:00Z".to_string(),
            body: MemoryBody("\
Spec 1: Alpha Architecture Overview
Spec 2: Beta Core Frequency and Power Profiles
Spec 3: Gamma Hardware Capabilities and Subsystems
\x1b[33m+-------------------------+\x1b[0m
|  ascii divider art      |
+-------------------------+
Spec 4: Delta Memory Allocation Table
Spec 5: Epsilon Thermal Constraints
Spec 6: Zeta Peripheral Interfaces
".to_string()),
            warning: None,
        };

        let file_path = tmp.join("moderate-memory.md");
        let orig_backup = tmp.join("moderate-memory.md.orig");

        // Write initial file to disk
        fs::write(&file_path, serialize_memory_file(&moderate)).unwrap();

        // Run save with auto_sanitize = true
        let (cleaned_body, outcome) = sanitize_memory_body(moderate.body.as_str());
        assert!(matches!(outcome, SanitizeOutcome::Cleaned { .. }));

        // Simulating save backup
        if matches!(outcome, SanitizeOutcome::Cleaned { .. }) {
            fs::copy(&file_path, &orig_backup).unwrap();
            let mut cleaned_mem = moderate.clone();
            cleaned_mem.body = MemoryBody(cleaned_body);
            fs::write(&file_path, serialize_memory_file(&cleaned_mem)).unwrap();
        }

        assert!(orig_backup.exists(), ".orig backup must be created beside sanitized file");
        let backup_content = fs::read_to_string(&orig_backup).unwrap();
        assert!(backup_content.contains("+-------------------------+"), ".orig backup must contain original art");
        let new_content = fs::read_to_string(&file_path).unwrap();
        assert!(!new_content.contains("+-------------------------+"), "Cleaned file must have art removed");
        assert!(new_content.contains("Spec 1: Alpha"));
        assert!(new_content.contains("Spec 3: Gamma"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    #[ignore]
    fn test_manual_run_real_frank_base_specs() {
        let real_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("foreman")
            .join("memory")
            .join("frank-base-specs.md");
        if real_path.exists() {
            let entry = parse_memory_file(&real_path).unwrap();
            let saved = save_memory_entry(&entry).unwrap();
            let backup = real_path.parent().unwrap().join("frank-base-specs.md.orig");
            assert!(backup.exists(), "Backup frank-base-specs.md.orig must exist beside original file");
            assert!(!saved.body.as_str().contains(".-------------------------:"), "CachyOS art must be removed");
            assert!(saved.body.as_str().contains("OS: CachyOS x86_64"), "OS spec must survive");
            assert!(saved.body.as_str().contains("CPU: AMD Ryzen AI 9 HX 370"), "CPU spec must survive");
            assert!(saved.body.as_str().contains("GPU: NVIDIA GeForce RTX 4050"), "GPU spec must survive");
            assert!(saved.body.as_str().contains("RAM: 32 GB LPDDR5x"), "RAM spec must survive");
            println!("Manual run against real frank-base-specs.md passed successfully!");
        }
    }
}
