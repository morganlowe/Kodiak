use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::{data_dir_path, runs_dir_path, task_run_dir_path};

static CURRENT_RUN_LOG_LEVEL: AtomicUsize = AtomicUsize::new(1); // 1 = Info

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunLogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl RunLogLevel {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "debug" => RunLogLevel::Debug,
            "warn" | "warning" => RunLogLevel::Warn,
            "error" | "bad" => RunLogLevel::Error,
            _ => RunLogLevel::Info,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RunLogLevel::Debug => "DEBUG",
            RunLogLevel::Info => "INFO",
            RunLogLevel::Warn => "WARN",
            RunLogLevel::Error => "ERROR",
        }
    }
}

pub fn set_current_run_log_level(level_str: &str) {
    let level = RunLogLevel::parse(level_str);
    CURRENT_RUN_LOG_LEVEL.store(level as usize, Ordering::Relaxed);
}

pub fn get_current_run_log_level() -> RunLogLevel {
    match CURRENT_RUN_LOG_LEVEL.load(Ordering::Relaxed) {
        0 => RunLogLevel::Debug,
        2 => RunLogLevel::Warn,
        3 => RunLogLevel::Error,
        _ => RunLogLevel::Info,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunMeta {
    pub task_id: String,
    pub prompt_hash: String,
    pub project: String,
    pub providers_used: Vec<String>,
    pub final_status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunLogResponse {
    pub task_id: String,
    pub content: String,
    pub exists: bool,
    pub pruned: bool,
    pub meta: Option<TaskRunMeta>,
}

/// Compute a compact prompt hash (e.g. first 12 hex chars of SHA-like simple digest).
pub fn compute_prompt_hash(prompt: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    prompt.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Appends a structured log line to `~/.local/share/kodiak/runs/<task_id>/run.log`
/// filtered against `configured_level`.
pub fn append_task_run_log(
    task_id: &str,
    stage: &str,
    level_str: &str,
    message: &str,
    configured_level_str: &str,
) {
    if task_id.trim().is_empty() {
        return;
    }

    let event_level = RunLogLevel::parse(level_str);
    let min_level = RunLogLevel::parse(configured_level_str);

    if event_level < min_level {
        return;
    }

    if let Ok(dir) = task_run_dir_path(task_id) {
        let log_file = dir.join("run.log");
        let timestamp = Utc::now().to_rfc3339();
        let line = format!(
            "[{timestamp}] [{stage}] [{}] {message}\n",
            event_level.as_str()
        );

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_file) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// Appends a stage transition with timing or status to the run log.
pub fn append_stage_transition_log(
    task_id: &str,
    stage: &str,
    status: &str,
    detail: Option<&str>,
    duration_secs: Option<f64>,
    configured_level_str: &str,
) {
    let mut msg = format!("Stage transition: stage='{stage}' status='{status}'");
    if let Some(d) = duration_secs {
        msg.push_str(&format!(" (duration: {d:.2}s)"));
    }
    if let Some(det) = detail {
        msg.push_str(&format!(" detail='{det}'"));
    }

    let level = if status == "failed" { "error" } else { "info" };
    append_task_run_log(task_id, stage, level, &msg, configured_level_str);
}

/// Writes or updates sidecar `meta.json` for a task run.
pub fn write_task_run_meta(task_id: &str, meta: &TaskRunMeta) -> Result<(), String> {
    let dir = task_run_dir_path(task_id)?;
    let meta_file = dir.join("meta.json");
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("Failed to serialize meta.json: {e}"))?;
    fs::write(&meta_file, json).map_err(|e| format!("Failed to write meta.json: {e}"))?;
    Ok(())
}

/// Reads sidecar `meta.json` if available.
pub fn read_task_run_meta(task_id: &str) -> Option<TaskRunMeta> {
    let dir = task_run_dir_path(task_id).ok()?;
    let meta_file = dir.join("meta.json");
    if !meta_file.exists() {
        return None;
    }
    let raw = fs::read_to_string(&meta_file).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Reads `run.log` and meta for the given task.
pub fn load_task_run_log(task_id: &str) -> Result<TaskRunLogResponse, String> {
    let dir = runs_dir_path()?.join(task_id);
    let log_file = dir.join("run.log");
    let meta = read_task_run_meta(task_id);

    if log_file.exists() {
        let content = fs::read_to_string(&log_file)
            .map_err(|e| format!("Failed to read run.log: {e}"))?;
        Ok(TaskRunLogResponse {
            task_id: task_id.to_string(),
            content,
            exists: true,
            pruned: false,
            meta,
        })
    } else {
        // Check if task exists in history.json to differentiate between pruned vs never existed
        let history_file = data_dir_path()?.join("history.json");
        let is_in_history = if history_file.exists() {
            fs::read_to_string(&history_file)
                .map(|text| text.contains(task_id))
                .unwrap_or(false)
        } else {
            false
        };

        if is_in_history {
            Ok(TaskRunLogResponse {
                task_id: task_id.to_string(),
                content: "Run log was pruned or is unavailable.".to_string(),
                exists: false,
                pruned: true,
                meta,
            })
        } else {
            Ok(TaskRunLogResponse {
                task_id: task_id.to_string(),
                content: "No run log found for this task.".to_string(),
                exists: false,
                pruned: false,
                meta,
            })
        }
    }
}

/// Prunes run logs to satisfy `max_runs` and `max_mb`.
/// IMPORTANT: NEVER touches `history.json` or task results!
pub fn prune_run_logs(runs_dir: &Path, max_runs: usize, max_mb: usize) -> Result<usize, String> {
    if !runs_dir.exists() {
        return Ok(0);
    }

    let mut entries = Vec::new();
    let read_dir = fs::read_dir(runs_dir)
        .map_err(|e| format!("Failed to read runs directory {}: {e}", runs_dir.display()))?;

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let metadata = entry.metadata().ok();
            let modified = metadata
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            // Compute directory size
            let mut dir_size = 0u64;
            if let Ok(files) = fs::read_dir(&path) {
                for f in files.flatten() {
                    if let Ok(fm) = f.metadata() {
                        dir_size += fm.len();
                    }
                }
            }

            entries.push((path, modified, dir_size));
        }
    }

    // Sort descending by modified time (newest first)
    entries.sort_by(|a, b| b.1.cmp(&a.1));

    let max_bytes = (max_mb as u64) * 1024 * 1024;
    let mut pruned_count = 0;
    let mut cumulative_size = 0u64;

    for (idx, (dir_path, _, dir_size)) in entries.iter().enumerate() {
        let exceeds_count = idx >= max_runs;
        cumulative_size += dir_size;
        let exceeds_size = cumulative_size > max_bytes;

        if exceeds_count || exceeds_size {
            if fs::remove_dir_all(dir_path).is_ok() {
                pruned_count += 1;
            }
        }
    }

    Ok(pruned_count)
}
