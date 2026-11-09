//! Centralized Tauri Event definitions for live pipeline stage progress, ticker logs, and streaming drafts (§2 & §4 Slice 2).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Single canonical Tauri event channel name used across backend and frontend.
pub const PIPELINE_EVENT_CHANNEL: &str = "kodiak://pipeline-event";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// Emitted when a pipeline stage (`interpret` | `execute` | `verify` | `done`) transitions state.
    StageChanged {
        task_id: String,
        stage: String,
        /// `"active"` | `"completed"` | `"failed"`
        status: String,
        detail: Option<String>,
        timestamp: String,
    },
    /// Real-time line appended to the Live Event Ticker.
    TickerLog {
        task_id: String,
        stage: String,
        /// `"info"` | `"route"` | `"ok"` | `"warn"` | `"bad"`
        level: String,
        message: String,
        timestamp: String,
    },
    /// Real-time token chunk streamed from local Ollama during `draft_code`.
    DraftStreamChunk {
        task_id: String,
        step_id: usize,
        title: String,
        action_class: String,
        target_path: String,
        model: String,
        chunk: String,
        accumulated: String,
    },
    /// Signals completion or failure of a streaming local draft.
    DraftStreamDone {
        task_id: String,
        step_id: usize,
        target_path: String,
        success: bool,
        error: Option<String>,
    },
}

pub fn emit_pipeline_event(app: Option<&AppHandle>, event: PipelineEvent) {
    if let Some(handle) = app {
        let _ = handle.emit(PIPELINE_EVENT_CHANNEL, &event);
    }
}

pub fn emit_stage_changed(
    app: Option<&AppHandle>,
    task_id: &str,
    stage: &str,
    status: &str,
    detail: Option<String>,
) {
    crate::run_log::append_stage_transition_log(
        task_id,
        stage,
        status,
        detail.as_deref(),
        None,
        crate::run_log::get_current_run_log_level().as_str(),
    );
    emit_pipeline_event(
        app,
        PipelineEvent::StageChanged {
            task_id: task_id.to_string(),
            stage: stage.to_string(),
            status: status.to_string(),
            detail,
            timestamp: Utc::now().to_rfc3339(),
        },
    );
}

pub fn emit_ticker_log(
    app: Option<&AppHandle>,
    task_id: &str,
    stage: &str,
    level: &str,
    message: impl Into<String>,
) {
    let msg = message.into();
    crate::run_log::append_task_run_log(
        task_id,
        stage,
        level,
        &msg,
        crate::run_log::get_current_run_log_level().as_str(),
    );
    emit_pipeline_event(
        app,
        PipelineEvent::TickerLog {
            task_id: task_id.to_string(),
            stage: stage.to_string(),
            level: level.to_string(),
            message: msg,
            timestamp: Utc::now().to_rfc3339(),
        },
    );
}
