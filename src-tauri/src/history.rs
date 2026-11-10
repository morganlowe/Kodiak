//! Task history persistence (`~/.local/share/kodiak/history.json`) (§5.7 & Slice 2 §3.2 Schema v2).

use crate::config::history_file_path;
use crate::enforcer::ActionClass;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const CURRENT_HISTORY_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    #[serde(default)]
    pub id: usize,
    #[serde(default)]
    pub title: String,
    pub action_class: ActionClass,
    #[serde(
        alias = "route_custom tool",
        alias = "route_custom_tool",
        alias = "tool",
        alias = "route"
    )]
    pub route_tool: String,
    #[serde(alias = "path", alias = "file", default)]
    pub target_path: String,
    #[serde(alias = "spec", alias = "command", default)]
    pub instruction: String,
    #[serde(default)]
    pub search_pattern: Option<String>,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub memories_used: Vec<String>,
    #[serde(default)]
    pub request_kind: Option<String>,
    #[serde(default)]
    pub justification: Option<String>,
    #[serde(default)]
    pub consequences_of_decline: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub answer_format: Option<String>,
    #[serde(default)]
    pub suggested_default: Option<String>,
}

impl Default for PlanStep {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            action_class: ActionClass::Read,
            route_tool: String::new(),
            target_path: String::new(),
            instruction: String::new(),
            search_pattern: None,
            start_line: None,
            end_line: None,
            memories_used: Vec::new(),
            request_kind: None,
            justification: None,
            consequences_of_decline: None,
            intent: None,
            question: None,
            answer_format: None,
            suggested_default: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub goal: String,
    pub estimated_risk: String,
    pub files_involved: Vec<String>,
    #[serde(default)]
    pub memories_used: Vec<String>,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecutionLog {
    pub step_id: usize,
    pub title: String,
    pub tier: String,
    pub model: String,
    pub route_tool: String,
    pub action_class: ActionClass,
    pub target_path: String,
    pub summary: String,
    pub status: String,
    #[serde(default)]
    pub warning_chip: Option<String>,
    #[serde(default)]
    pub memory_delivery_chip: Option<String>,
}

impl Default for StepExecutionLog {
    fn default() -> Self {
        Self {
            step_id: 0,
            title: String::new(),
            tier: String::new(),
            model: String::new(),
            route_tool: String::new(),
            action_class: ActionClass::Read,
            target_path: String::new(),
            summary: String::new(),
            status: String::new(),
            warning_chip: None,
            memory_delivery_chip: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCard {
    pub card_id: String,
    pub step_id: usize,
    pub title: String,
    pub action_class: ActionClass,
    pub target_path: String,
    pub directory: String,
    pub grant_level: String,
    ///Unified diff for file edits or command preview for shell/mkdir
    pub diff_or_command: String,
    /// Full new content when action_class is Create or Overwrite
    #[serde(default)]
    pub proposed_content: Option<String>,
    /// Shell command when action_class is Execute
    #[serde(default)]
    pub shell_command: Option<String>,
    /// "pending_approval" | "approved" | "rejected" | "denied" | "auto_executed_grant" | "dry_run"
    pub status: String,
    pub decision_reason: String,
    #[serde(default)]
    pub execution_output: Option<String>,
    #[serde(default)]
    pub warning_chip: Option<String>,
    #[serde(default)]
    pub request_kind: Option<String>,
    #[serde(default)]
    pub justification: Option<String>,
    #[serde(default)]
    pub consequences_of_decline: Option<String>,
    #[serde(default)]
    pub user_provided_info: Option<String>,
    #[serde(default)]
    pub removal_ratio: Option<f64>,
    #[serde(default)]
    pub inconsistency_note: Option<String>,
    #[serde(default)]
    pub memory_delivery_chip: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub answer_format: Option<String>,
    #[serde(default)]
    pub suggested_default: Option<String>,
}

impl Default for ActionCard {
    fn default() -> Self {
        Self {
            card_id: String::new(),
            step_id: 0,
            title: String::new(),
            action_class: ActionClass::Read,
            target_path: String::new(),
            directory: String::new(),
            grant_level: String::new(),
            diff_or_command: String::new(),
            proposed_content: None,
            shell_command: None,
            status: String::new(),
            decision_reason: String::new(),
            execution_output: None,
            warning_chip: None,
            request_kind: None,
            justification: None,
            consequences_of_decline: None,
            user_provided_info: None,
            removal_ratio: None,
            inconsistency_note: None,
            memory_delivery_chip: None,
            question: None,
            answer_format: None,
            suggested_default: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// "pass" | "concerns" | "fail"
    pub verdict: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceCard {
    pub card_id: String,
    pub stage: String,
    pub error_text: String,
    pub timestamp: String,
    /// "pending" | "resolved"
    pub status: String,
    /// "defer" | "local_interpret" | "local_verify" | "alt_provider"
    pub chosen_option: Option<String>,
    pub chosen_at: Option<String>,
    pub chosen_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: String,
    pub timestamp: String,
    /// Auto-titled from `interpret` stage's `goal` field (or fallback to `prompt`) (§3.1 Slice 2)
    #[serde(default)]
    pub title: String,
    /// Pinned favorite status in sidebar (§3.1 Slice 2)
    #[serde(default)]
    pub starred: bool,
    pub prompt: String,
    pub project_dir: String,
    pub cloud_model: String,
    pub cruise_control: bool,
    pub dry_run: bool,
    pub posture_summary: String,
    pub auto_executed_count: usize,
    pub queued_count: usize,
    pub denied_count: usize,
    pub plan: Option<TaskPlan>,
    pub step_logs: Vec<StepExecutionLog>,
    pub action_cards: Vec<ActionCard>,
    pub verification: Option<VerificationResult>,
    pub failed_stage: Option<String>,
    pub stage_error: Option<String>,
    pub journal_path: String,
    #[serde(default)]
    pub task_scoped_grants: Vec<crate::config::WhitelistEntry>,
    #[serde(default)]
    pub task_provided_info: Vec<String>,
    #[serde(default)]
    pub stage_error_kind: Option<String>,
    #[serde(default)]
    pub choice_cards: Vec<ChoiceCard>,
    #[serde(default)]
    pub deferred: bool,
    #[serde(default)]
    pub deferred_stage: Option<String>,
    #[serde(default)]
    pub defer_reason: Option<String>,
    #[serde(default)]
    pub local_grade_plan: bool,
    #[serde(default)]
    pub local_grade_verify: bool,
    #[serde(default)]
    pub provider_interpret: Option<String>,
    #[serde(default)]
    pub provider_verify: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStoreV2 {
    pub schema_version: u32,
    pub tasks: Vec<TaskRecord>,
}

pub fn normalize_task_record(mut rec: TaskRecord) -> TaskRecord {
    let goal_title = rec
        .plan
        .as_ref()
        .map(|p| p.goal.trim().to_string())
        .filter(|g| !g.is_empty());

    if let Some(gt) = goal_title {
        rec.title = gt;
    } else if rec.title.trim().is_empty() {
        rec.title = rec.prompt.trim().to_string();
    }
    rec
}

pub fn load_history_from_path(path: &Path) -> Result<Vec<TaskRecord>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let val: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    match val {
        Value::Array(_) => {
            // Slice 1 legacy format: bare Vec<TaskRecord> -> migrate to Schema v2
            let v1_records: Vec<TaskRecord> = serde_json::from_value(val)
                .map_err(|e| format!("Failed to parse Slice 1 history in {}: {e}", path.display()))?;
            let migrated: Vec<TaskRecord> = v1_records
                .into_iter()
                .map(normalize_task_record)
                .collect();
            let _ = save_history_to_path(path, &migrated);
            Ok(migrated)
        }
        Value::Object(_) => {
            let store: HistoryStoreV2 = serde_json::from_value(val)
                .map_err(|e| format!("Failed to parse Schema v2 history in {}: {e}", path.display()))?;
            let tasks: Vec<TaskRecord> = store
                .tasks
                .into_iter()
                .map(normalize_task_record)
                .collect();
            Ok(tasks)
        }
        _ => Err(format!("Unexpected JSON root in {}", path.display())),
    }
}

pub fn save_history_to_path(path: &Path, records: &[TaskRecord]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let normalized: Vec<TaskRecord> = records
        .iter()
        .cloned()
        .map(normalize_task_record)
        .collect();
    let store = HistoryStoreV2 {
        schema_version: CURRENT_HISTORY_SCHEMA_VERSION,
        tasks: normalized,
    };
    let serialized = serde_json::to_string_pretty(&store)
        .map_err(|e| format!("Failed to serialize history v2: {e}"))?;
    fs::write(path, serialized)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(())
}

static HISTORY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load_history() -> Result<Vec<TaskRecord>, String> {
    let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
    let path = history_file_path()?;
    load_history_from_path(&path)
}

pub fn save_history(records: &[TaskRecord]) -> Result<(), String> {
    let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
    let path = history_file_path()?;
    save_history_to_path(&path, records)
}

pub fn upsert_task_record(record: &TaskRecord) -> Result<(), String> {
    let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
    let path = history_file_path()?;
    let mut all = load_history_from_path(&path).unwrap_or_default();
    let mut normalized = normalize_task_record(record.clone());
    if let Some(existing) = all.iter_mut().find(|r| r.task_id == normalized.task_id) {
        normalized.starred = existing.starred || normalized.starred;
        *existing = normalized;
    } else {
        all.insert(0, normalized);
    }
    save_history_to_path(&path, &all)
}

pub fn toggle_star_in_history(task_id: &str) -> Result<TaskRecord, String> {
    let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
    let path = history_file_path()?;
    let mut all = load_history_from_path(&path)?;
    let updated = {
        let rec = all
            .iter_mut()
            .find(|r| r.task_id == task_id)
            .ok_or_else(|| format!("Task {task_id} not found"))?;
        rec.starred = !rec.starred;
        rec.clone()
    };
    save_history_to_path(&path, &all)?;
    Ok(updated)
}

pub fn delete_task_from_history(task_id: &str) -> Result<(TaskRecord, Vec<TaskRecord>), String> {
    let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
    let path = history_file_path()?;
    let mut all = load_history_from_path(&path)?;
    let pos = all
        .iter()
        .position(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;
    let removed = all.remove(pos);
    save_history_to_path(&path, &all)?;
    Ok((removed, all))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v1_array_migrates_to_v2_without_data_loss() {
        let dir = std::env::temp_dir().join("foreman_hist_migrate_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let v1 = r#"[{"task_id":"abc","timestamp":"2026-11-07T11:44:05Z","prompt":"do a thing","project_dir":"/tmp/p","cloud_model":"lumo","cruise_control":false,"dry_run":false,"posture_summary":"Manual approval","auto_executed_count":0,"queued_count":0,"denied_count":0,"plan":{"goal":"Make the thing","estimated_risk":"low","files_involved":[],"steps":[]},"step_logs":[],"action_cards":[],"verification":null,"failed_stage":"verify","stage_error":"boom","journal_path":"/tmp/j"}]"#;
        fs::write(&path, v1).unwrap();
        let recs = load_history_from_path(&path).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].task_id, "abc");
        assert_eq!(recs[0].title, "Make the thing");
        assert!(!recs[0].starred);
        assert_eq!(recs[0].stage_error.as_deref(), Some("boom"));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"schema_version\": 2"));
        assert_eq!(load_history_from_path(&path).unwrap().len(), 1);
    }
}
