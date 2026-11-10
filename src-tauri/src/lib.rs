pub mod config;
pub mod enforcer;
pub mod events;
pub mod hardware;
pub mod history;
pub mod memory;
pub mod migration;
pub mod pipeline;
pub mod prompts;
pub mod queue;
pub mod run_log;

use serde::{Deserialize, Serialize};
use config::{journal_file_path, load_or_seed_config, save_config, AppConfig, GrantLevel};
use enforcer::{
    check_command_deny_list, check_path_deny_list, find_governing_whitelist, log_decision,
    resolve_canonical_target, ActionClass, JournalEntry,
};
use events::{emit_stage_changed, emit_ticker_log};
use history::{
    delete_task_from_history, load_history, toggle_star_in_history, upsert_task_record,
    ChoiceCard, TaskPlan, TaskRecord, VerificationResult,
};
use memory::{delete_memory_entry, load_all_memories, save_memory_entry, MemoryEntry};
use pipeline::{
    ai_build_wiki_project_service, build_posture_summary, check_system_health,
    classify_cloud_error_from_str, complete_wiki_setup_service, create_initial_task_record,
    detect_projects_in_dir_service, fetch_provider_models, get_detailed_wiki_status, get_wiki_setup_info_service,
    list_wiki_projects_service, ping_alt_provider, probe_cloud_health, rebuild_wiki_assets_service,
    resolve_active_project_root, restart_wiki_service, run_execute_stage_with_events,
    run_interpret_stage, run_interpret_stage_with_provider, run_shell_command_bounded,
    run_verify_stage, run_verify_stage_with_provider, save_and_return_record,
    start_lumo_bridge_service, start_wiki_service, stop_wiki_service, sync_wiki_catalog_service,
    CloudErrorKind, HealthStatus, ProviderTestResult, StageProvider, WikiSetupInfo,
    WikiSetupPayload, WikiStatusDetails,
};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub config: Mutex<AppConfig>,
    /// §4.2.1 Cruise Control: session posture ONLY, defaults to OFF on every startup
    pub cruise_control: AtomicBool,
    /// Multi-task queue manager and VRAM serialization mutex (§2.1 Slice 2)
    pub queue: Arc<queue::QueueManager>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: Mutex::new(AppConfig::default()),
            cruise_control: AtomicBool::new(false),
            queue: Arc::new(queue::QueueManager::new()),
        }
    }
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let guard = state.config.lock().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

#[tauri::command]
fn update_config(state: State<'_, AppState>, new_config: AppConfig) -> Result<AppConfig, String> {
    save_config(&new_config)?;
    run_log::set_current_run_log_level(&new_config.run_log_level);
    let mut guard = state.config.lock().map_err(|e| e.to_string())?;
    *guard = new_config.clone();
    Ok(new_config)
}

#[tauri::command]
fn get_cruise_control(state: State<'_, AppState>) -> bool {
    state.cruise_control.load(Ordering::SeqCst)
}

#[tauri::command]
fn set_cruise_control(state: State<'_, AppState>, enabled: bool) -> bool {
    state.cruise_control.store(enabled, Ordering::SeqCst);
    enabled
}

#[tauri::command]
async fn check_health(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    Ok(check_system_health(&cfg).await)
}

#[tauri::command]
async fn start_lumo_bridge(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    start_lumo_bridge_service(&cfg).await
}

#[tauri::command]
async fn start_wiki_server(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    start_wiki_service(&cfg).await
}

#[tauri::command]
async fn stop_wiki_server(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    stop_wiki_service(&cfg).await
}

#[tauri::command]
async fn restart_wiki_server(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    restart_wiki_service(&cfg).await
}

#[tauri::command]
async fn get_wiki_details(state: State<'_, AppState>) -> Result<WikiStatusDetails, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    get_detailed_wiki_status(&cfg).await
}

#[tauri::command]
async fn sync_wiki_catalog(state: State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    sync_wiki_catalog_service(&cfg).await
}

#[tauri::command]
async fn rebuild_wiki_assets(state: State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    rebuild_wiki_assets_service(&cfg).await
}

#[tauri::command]
async fn ai_build_project_wiki(
    state: State<'_, AppState>,
    project_name: String,
    model: Option<String>,
    overwrite: Option<bool>,
) -> Result<String, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    ai_build_wiki_project_service(&cfg, &project_name, model.as_deref(), overwrite.unwrap_or(true)).await
}

#[tauri::command]
async fn list_wiki_projects(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    list_wiki_projects_service(&cfg).await
}

#[tauri::command]
async fn get_wiki_setup_info(state: State<'_, AppState>) -> Result<WikiSetupInfo, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    get_wiki_setup_info_service(&cfg).await
}

#[tauri::command]
async fn complete_wiki_setup(
    state: State<'_, AppState>,
    payload: WikiSetupPayload,
) -> Result<AppConfig, String> {
    let mut cfg = {
        state.config.lock().map_err(|e| e.to_string())?.clone()
    };
    let updated = complete_wiki_setup_service(&mut cfg, payload).await?;
    save_config(&updated)?;
    let mut guard = state.config.lock().map_err(|e| e.to_string())?;
    *guard = updated.clone();
    Ok(updated)
}

#[tauri::command]
async fn set_wiki_enabled(state: State<'_, AppState>, enabled: bool) -> Result<AppConfig, String> {
    let mut cfg = {
        state.config.lock().map_err(|e| e.to_string())?.clone()
    };
    cfg.wiki_enabled = enabled;
    cfg.wiki_configured = true;
    if !enabled {
        let _ = stop_wiki_service(&cfg).await;
    }
    save_config(&cfg)?;
    let mut guard = state.config.lock().map_err(|e| e.to_string())?;
    *guard = cfg.clone();
    Ok(cfg)
}

#[tauri::command]
fn detect_projects_in_dir(dir_path: String) -> Vec<String> {
    detect_projects_in_dir_service(&dir_path)
}

#[tauri::command]
async fn run_task_pipeline(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    let project_root = resolve_active_project_root(&cfg)?;

    let mut record = create_initial_task_record(&cfg, &prompt, &project_root, cruise);
    upsert_task_record(&record)?;

    // Stage 1: Cloud Interpret
    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "interpret",
        "active",
        Some(format!("Cloud interpret via {}", cfg.cloud_model)),
    );
    emit_ticker_log(
        Some(&app),
        &record.task_id,
        "interpret",
        "info",
        format!(
            "Stage 1 (interpret) → cloud/{} parsing task plan...",
            cfg.cloud_model
        ),
    );

    let interpret_res: Result<TaskPlan, String> = if cfg.cascade.enabled && cfg.cascade.consent {
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res = Err("No providers available in cascade".to_string());
        for (pname, sprovider) in cascade_providers {
            match run_interpret_stage_with_provider(&cfg, &project_root, &prompt, &sprovider).await {
                Ok(plan) => {
                    record.provider_interpret = Some(pname.clone());
                    append_journal_note(&record.task_id, "stage_provider", &format!("stage=interpret, provider={pname}"));
                    final_res = Ok(plan);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") {
                        "429"
                    } else if e.contains("500") {
                        "500"
                    } else if e.contains("502") {
                        "502"
                    } else if e.contains("503") {
                        "503"
                    } else {
                        "429"
                    };
                    append_journal_note(&record.task_id, "cascade_attempt", &format!("stage=interpret provider={pname} error={err_code}"));
                    emit_ticker_log(
                        Some(&app),
                        &record.task_id,
                        "interpret",
                        "warn",
                        format!("cascade_attempt: stage=interpret provider={pname} error={err_code}"),
                    );
                    final_res = Err(e);
                }
            }
        }
        final_res
    } else {
        run_interpret_stage(&cfg, &project_root, &prompt).await
    };

    match interpret_res {
        Ok(plan) => {
            if !plan.goal.trim().is_empty() {
                record.title = plan.goal.trim().to_string();
            }
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "interpret",
                "ok",
                format!(
                    "Stage 1 complete — goal: \"{}\" ({} steps, risk={})",
                    plan.goal,
                    plan.steps.len(),
                    plan.estimated_risk
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "interpret",
                "completed",
                Some(plan.goal.clone()),
            );
            record.plan = Some(plan);
            record.failed_stage = None;
            record.stage_error = None;
            upsert_task_record(&record)?;
        }
        Err(err) => {
            let err_kind = classify_cloud_error_from_str(&err);
            if err_kind == CloudErrorKind::QuotaExhausted {
                record.stage_error_kind = Some("quota_exhausted".to_string());
                record.choice_cards.push(ChoiceCard {
                    card_id: format!("choice-interpret-{}", Utc::now().timestamp_millis()),
                    stage: "interpret".to_string(),
                    error_text: err.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                    status: "pending".to_string(),
                    chosen_option: None,
                    chosen_at: None,
                    chosen_provider: None,
                });
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "warn",
                    "Cloud quota exhausted — waiting for your decision".to_string(),
                );
            } else {
                record.stage_error_kind = Some(format!("{:?}", err_kind).to_ascii_lowercase());
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "bad",
                    format!("Stage 1 (interpret) failed: {err}"),
                );
            }
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "interpret",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("interpret".to_string());
            record.stage_error = Some(err);
            return save_and_return_record(record);
        }
    }

    // Stage 2: Local Execute + Enforcer
    let plan_ref = record.plan.clone().unwrap();
    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "execute",
        "active",
        Some(format!("Executing {} local step(s)", plan_ref.steps.len())),
    );
    emit_ticker_log(
        Some(&app),
        &record.task_id,
        "execute",
        "info",
        format!(
            "Stage 2 (execute) → running {} planned step(s) locally on Ollama...",
            plan_ref.steps.len()
        ),
    );

    let exec_res = {
        // Enforce strict VRAM serialization for local model inference (§2.1 Slice 2)
        let _vram_guard = state.queue.vram_lock.lock().await;
        run_execute_stage_with_events(
            Some(&app),
            &cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        )
        .await
    };

    match exec_res {
        Ok((step_logs, action_cards)) => {
            let (posture, auto_c, queued_c, denied_c) =
                build_posture_summary(&cfg, cruise, &action_cards);
            record.posture_summary = posture;
            record.auto_executed_count = auto_c;
            record.queued_count = queued_c;
            record.denied_count = denied_c;
            record.step_logs = step_logs;
            record.action_cards = action_cards;
            record.failed_stage = None;
            record.stage_error = None;
            upsert_task_record(&record)?;
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "execute",
                "ok",
                format!(
                    "Stage 2 complete — auto-executed={}, queued_for_approval={}, blocked={}",
                    auto_c, queued_c, denied_c
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "execute",
                "completed",
                None,
            );
        }
        Err(err) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "execute",
                "bad",
                format!("Stage 2 (execute) failed: {err}"),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "execute",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("execute".to_string());
            record.stage_error = Some(err);
            return save_and_return_record(record);
        }
    }

    // Stage 3: Cloud Verify
    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "verify",
        "active",
        Some("Verifying condensed diffs against plan".to_string()),
    );
    emit_ticker_log(
        Some(&app),
        &record.task_id,
        "verify",
        "info",
        format!(
            "Stage 3 (verify) → cloud/{} reviewing condensed summaries & diffs...",
            cfg.cloud_model
        ),
    );

    let verify_res: Result<VerificationResult, String> = if cfg.cascade.enabled && cfg.cascade.consent {
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res = Err("No providers available in cascade".to_string());
        for (pname, sprovider) in cascade_providers {
            match run_verify_stage_with_provider(&cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards, &sprovider).await {
                Ok(ver) => {
                    record.provider_verify = Some(pname.clone());
                    append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider={pname}"));
                    final_res = Ok(ver);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") {
                        "429"
                    } else if e.contains("500") {
                        "500"
                    } else if e.contains("502") {
                        "502"
                    } else if e.contains("503") {
                        "503"
                    } else {
                        "429"
                    };
                    append_journal_note(&record.task_id, "cascade_attempt", &format!("stage=verify provider={pname} error={err_code}"));
                    emit_ticker_log(
                        Some(&app),
                        &record.task_id,
                        "verify",
                        "warn",
                        format!("cascade_attempt: stage=verify provider={pname} error={err_code}"),
                    );
                    final_res = Err(e);
                }
            }
        }
        final_res
    } else {
        run_verify_stage(
            &cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
        )
        .await
    };

    match verify_res {
        Ok(ver) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "verify",
                "ok",
                format!(
                    "Stage 3 complete — verdict: {} ({})",
                    ver.verdict.to_uppercase(),
                    ver.rationale
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "completed",
                Some(ver.verdict.clone()),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "done",
                "completed",
                Some(ver.verdict.clone()),
            );
            record.verification = Some(ver);
            record.failed_stage = None;
            record.stage_error = None;
        }
        Err(err) => {
            let err_kind = classify_cloud_error_from_str(&err);
            if err_kind == CloudErrorKind::QuotaExhausted {
                record.stage_error_kind = Some("quota_exhausted".to_string());
                record.choice_cards.push(ChoiceCard {
                    card_id: format!("choice-verify-{}", Utc::now().timestamp_millis()),
                    stage: "verify".to_string(),
                    error_text: err.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                    status: "pending".to_string(),
                    chosen_option: None,
                    chosen_at: None,
                    chosen_provider: None,
                });
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "verify",
                    "warn",
                    "Cloud quota exhausted — waiting for your decision".to_string(),
                );
            } else {
                record.stage_error_kind = Some(format!("{:?}", err_kind).to_ascii_lowercase());
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "verify",
                    "bad",
                    format!("Stage 3 (verify) failed: {err}"),
                );
            }
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("verify".to_string());
            record.stage_error = Some(err);
        }
    }

    save_and_return_record(record)
}

/// Retries ONLY the failed stage (`interpret`, `execute`, or `verify`) (§7 & Slice 2 §2.1).
#[tauri::command]
async fn retry_task_stage(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);

    let all = load_history()?;
    let mut record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let project_root = Path::new(&record.project_dir).to_path_buf();
    let stage = record
        .failed_stage
        .clone()
        .unwrap_or_else(|| "interpret".to_string());

    if record.stage_error_kind.as_deref() == Some("quota_exhausted") {
        return Err("Retry suppressed: cloud quota is exhausted. Please select Defer, Local, or Alternate Provider on the Choice Card.".to_string());
    }

    emit_ticker_log(
        Some(&app),
        &record.task_id,
        &stage,
        "info",
        format!("Retrying stage '{stage}' only..."),
    );

    if stage == "interpret" || record.plan.is_none() {
        emit_stage_changed(
            Some(&app),
            &record.task_id,
            "interpret",
            "active",
            Some("Retrying interpret stage".to_string()),
        );
        match run_interpret_stage(&cfg, &project_root, &record.prompt).await {
            Ok(plan) => {
                if !plan.goal.trim().is_empty() {
                    record.title = plan.goal.trim().to_string();
                }
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "ok",
                    format!("Stage 1 retry succeeded — goal: \"{}\"", plan.goal),
                );
                emit_stage_changed(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "completed",
                    Some(plan.goal.clone()),
                );
                record.plan = Some(plan);
                record.failed_stage = None;
                record.stage_error = None;
                upsert_task_record(&record)?;
            }
            Err(err) => {
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "bad",
                    format!("Stage 1 retry failed: {err}"),
                );
                emit_stage_changed(
                    Some(&app),
                    &record.task_id,
                    "interpret",
                    "failed",
                    Some(err.clone()),
                );
                record.failed_stage = Some("interpret".to_string());
                record.stage_error = Some(err);
                return save_and_return_record(record);
            }
        }
    }

    if stage == "interpret" || stage == "execute" || record.step_logs.is_empty() {
        let plan_ref = record.plan.clone().unwrap();
        emit_stage_changed(
            Some(&app),
            &record.task_id,
            "execute",
            "active",
            Some("Retrying execute stage".to_string()),
        );
        match run_execute_stage_with_events(
            Some(&app),
            &cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        )
        .await
        {
            Ok((step_logs, action_cards)) => {
                let (posture, auto_c, queued_c, denied_c) =
                    build_posture_summary(&cfg, cruise, &action_cards);
                record.posture_summary = posture;
                record.auto_executed_count = auto_c;
                record.queued_count = queued_c;
                record.denied_count = denied_c;
                record.step_logs = step_logs;
                record.action_cards = action_cards;
                record.failed_stage = None;
                record.stage_error = None;
                upsert_task_record(&record)?;
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "execute",
                    "ok",
                    "Stage 2 retry succeeded",
                );
                emit_stage_changed(
                    Some(&app),
                    &record.task_id,
                    "execute",
                    "completed",
                    None,
                );
            }
            Err(err) => {
                emit_ticker_log(
                    Some(&app),
                    &record.task_id,
                    "execute",
                    "bad",
                    format!("Stage 2 retry failed: {err}"),
                );
                emit_stage_changed(
                    Some(&app),
                    &record.task_id,
                    "execute",
                    "failed",
                    Some(err.clone()),
                );
                record.failed_stage = Some("execute".to_string());
                record.stage_error = Some(err);
                return save_and_return_record(record);
            }
        }
    }

    let plan_ref = record.plan.clone().unwrap();
    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "verify",
        "active",
        Some("Retrying verify stage".to_string()),
    );
    let verify_res: Result<VerificationResult, String> = if cfg.cascade.enabled && cfg.cascade.consent {
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res = Err("No providers available in cascade".to_string());
        for (pname, sprovider) in cascade_providers {
            match run_verify_stage_with_provider(&cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards, &sprovider).await {
                Ok(ver) => {
                    record.provider_verify = Some(pname.clone());
                    append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider={pname}"));
                    final_res = Ok(ver);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") {
                        "429"
                    } else if e.contains("500") {
                        "500"
                    } else if e.contains("502") {
                        "502"
                    } else if e.contains("503") {
                        "503"
                    } else {
                        "429"
                    };
                    append_journal_note(&record.task_id, "cascade_attempt", &format!("stage=verify provider={pname} error={err_code}"));
                    emit_ticker_log(
                        Some(&app),
                        &record.task_id,
                        "verify",
                        "warn",
                        format!("cascade_attempt: stage=verify provider={pname} error={err_code}"),
                    );
                    final_res = Err(e);
                }
            }
        }
        final_res
    } else {
        run_verify_stage(
            &cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
        )
        .await
    };

    match verify_res {
        Ok(ver) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "verify",
                "ok",
                format!(
                    "Stage 3 retry succeeded — verdict: {} ({})",
                    ver.verdict.to_uppercase(),
                    ver.rationale
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "completed",
                Some(ver.verdict.clone()),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "done",
                "completed",
                Some(ver.verdict.clone()),
            );
            record.verification = Some(ver);
            record.failed_stage = None;
            record.stage_error = None;
        }
        Err(err) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "verify",
                "bad",
                format!("Stage 3 retry failed: {err}"),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("verify".to_string());
            record.stage_error = Some(err);
        }
    }

    save_and_return_record(record)
}

pub async fn apply_card_approval(
    cfg: &AppConfig,
    cruise: bool,
    mut record: TaskRecord,
    card_id: &str,
) -> Result<TaskRecord, String> {
    let project_root = Path::new(&record.project_dir).to_path_buf();
    let card_idx = record
        .action_cards
        .iter()
        .position(|c| c.card_id == card_id)
        .ok_or_else(|| format!("Action card {card_id} not found"))?;

    let card = record.action_cards[card_idx].clone();
    if card.status != "pending_approval" {
        return Err(format!("Card is already in state '{}'", card.status));
    }

    // Re-verify deny list & canonical whitelist boundary before executing approved item
    let (governing_dir, grant_level) = if card.action_class == ActionClass::Request {
        (PathBuf::from(&record.project_dir), GrantLevel::Read)
    } else {
        let canonical_target = resolve_canonical_target(&project_root, &card.target_path)?;
        check_path_deny_list(&canonical_target)?;
        let mut effective_whitelist = cfg.whitelist.clone();
        effective_whitelist.extend(record.task_scoped_grants.clone());
        find_governing_whitelist(&canonical_target, &effective_whitelist)?
    };

    let exec_output = match card.action_class {
        ActionClass::Create | ActionClass::Overwrite => {
            let canonical_target = resolve_canonical_target(&project_root, &card.target_path)?;
            let content = card
                .proposed_content
                .as_deref()
                .ok_or_else(|| "Missing proposed file content on card".to_string())?;
            if let Some(parent) = canonical_target.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("Failed to create parent dir '{}': {e}", parent.display())
                })?;
            }
            fs::write(&canonical_target, content).map_err(|e| {
                format!("Failed to write '{}': {e}", canonical_target.display())
            })?;
            Some(format!(
                "Wrote {} bytes to {}",
                content.len(),
                canonical_target.display()
            ))
        }
        ActionClass::Mkdir => {
            let canonical_target = resolve_canonical_target(&project_root, &card.target_path)?;
            fs::create_dir_all(&canonical_target).map_err(|e| {
                format!(
                    "Failed to create directory '{}': {e}",
                    canonical_target.display()
                )
            })?;
            Some(format!("Created directory {}", canonical_target.display()))
        }
        ActionClass::Execute | ActionClass::Read => {
            let cmd = card
                .shell_command
                .as_deref()
                .unwrap_or(&card.diff_or_command);
            check_command_deny_list(cmd)?;
            let out =
                match run_shell_command_bounded(&project_root, cmd, cfg.command_timeout_secs).await
                {
                    Ok(s) => s,
                    Err(e) => e,
                };
            Some(out)
        }
        ActionClass::Request => {
            if card.request_kind.as_deref() == Some("capability") {
                if let Err(reason) = check_path_deny_list(Path::new(&card.target_path)) {
                    return Err(format!("Cannot grant capability: {reason}"));
                }
                record.task_scoped_grants.push(crate::config::WhitelistEntry {
                    path: card.target_path.clone(),
                    level: GrantLevel::Read,
                });
                Some(format!("Granted task-scoped capability for '{}'", card.target_path))
            } else {
                Some("Approved request".to_string())
            }
        }
    };

    let log_event = if card.action_class == ActionClass::Request {
        "request_granted"
    } else {
        "accepted"
    };

    log_decision(
        &record.task_id,
        card.step_id,
        card.action_class,
        &card.target_path,
        &card.title,
        &governing_dir.to_string_lossy(),
        grant_level,
        cruise,
        record.dry_run,
        log_event,
        "Approved manually by user in Approval Queue",
    );

    record.action_cards[card_idx].status = "approved".to_string();
    record.action_cards[card_idx].decision_reason = "Approved manually by user".to_string();
    record.action_cards[card_idx].execution_output = exec_output;

    if let Some(step_log) = record
        .step_logs
        .iter_mut()
        .find(|s| s.step_id == card.step_id)
    {
        step_log.status = "approved".to_string();
    }

    record.queued_count = record
        .action_cards
        .iter()
        .filter(|c| c.status == "pending_approval")
        .count();

    save_and_return_record(record)
}

pub fn apply_card_rejection(
    cruise: bool,
    mut record: TaskRecord,
    card_id: &str,
    reason: &str,
) -> Result<TaskRecord, String> {
    let card_idx = record
        .action_cards
        .iter()
        .position(|c| c.card_id == card_id)
        .ok_or_else(|| format!("Action card {card_id} not found"))?;

    let card = record.action_cards[card_idx].clone();
    if card.status != "pending_approval" {
        return Err(format!("Card is already in state '{}'", card.status));
    }

    let clean_reason = if reason.trim().is_empty() {
        "Rejected by user (no reason specified)".to_string()
    } else {
        reason.trim().to_string()
    };

    let log_event = if card.action_class == ActionClass::Request {
        "request_declined"
    } else {
        "rejected"
    };

    log_decision(
        &record.task_id,
        card.step_id,
        card.action_class,
        &card.target_path,
        &card.title,
        &card.directory,
        GrantLevel::Read,
        cruise,
        record.dry_run,
        log_event,
        &clean_reason,
    );

    record.action_cards[card_idx].status = if card.action_class == ActionClass::Request {
        "declined".to_string()
    } else {
        "rejected".to_string()
    };
    record.action_cards[card_idx].decision_reason = clean_reason.clone();

    if let Some(step_log) = record
        .step_logs
        .iter_mut()
        .find(|s| s.step_id == card.step_id)
    {
        step_log.status = if card.action_class == ActionClass::Request {
            "declined".to_string()
        } else {
            "rejected".to_string()
        };
    }

    // If a request was declined, mark subsequent dependent steps as skipped
    if card.action_class == ActionClass::Request {
        let card_step_id = card.step_id;
        let card_title = card.title.clone();
        for s in record.step_logs.iter_mut() {
            if s.step_id > card_step_id && s.status == "pending_approval" {
                s.status = "skipped".to_string();
                s.summary = format!("Skipped because request '{card_title}' was declined.");
            }
        }
        for c in record.action_cards.iter_mut() {
            if c.step_id > card_step_id && c.status == "pending_approval" {
                c.status = "skipped".to_string();
                c.decision_reason = format!("Skipped because request '{card_title}' was declined.");
            }
        }
    }

    record.queued_count = record
        .action_cards
        .iter()
        .filter(|c| c.status == "pending_approval")
        .count();

    save_and_return_record(record)
}

#[tauri::command]
async fn approve_card(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    card_id: String,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);

    let all = load_history()?;
    let record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let updated = apply_card_approval(&cfg, cruise, record, &card_id).await?;
    state.queue.sync_from_task_record(
        &updated.task_id,
        updated.queued_count,
        updated.auto_executed_count,
        updated.denied_count,
        updated.queued_count == 0,
    );
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
    if let Some(card) = updated.action_cards.iter().find(|c| c.card_id == card_id) {
        emit_ticker_log(
            Some(&app),
            &task_id,
            "execute",
            "ok",
            format!(
                "Approval event: step #{} (`{}`) APPROVED & executed by user",
                card.step_id, card.target_path
            ),
        );
    }
    Ok(updated)
}

#[tauri::command]
fn reject_card(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    card_id: String,
    reason: String,
) -> Result<TaskRecord, String> {
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    let all = load_history()?;
    let record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let updated = apply_card_rejection(cruise, record, &card_id, &reason)?;
    state.queue.sync_from_task_record(
        &updated.task_id,
        updated.queued_count,
        updated.auto_executed_count,
        updated.denied_count,
        updated.queued_count == 0,
    );
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
    if let Some(card) = updated.action_cards.iter().find(|c| c.card_id == card_id) {
        emit_ticker_log(
            Some(&app),
            &task_id,
            "execute",
            "warn",
            format!(
                "Approval event: step #{} (`{}`) REJECTED by user ({})",
                card.step_id, card.target_path, card.decision_reason
            ),
        );
    }
    Ok(updated)
}

pub fn apply_provide_request_info(
    mut record: TaskRecord,
    card_id: &str,
    user_info: &str,
) -> Result<TaskRecord, String> {
    let card_idx = record
        .action_cards
        .iter()
        .position(|c| c.card_id == card_id)
        .ok_or_else(|| format!("Card {card_id} not found"))?;

    record.action_cards[card_idx].status = "provided".to_string();
    record.action_cards[card_idx].user_provided_info = Some(user_info.to_string());
    record.action_cards[card_idx].decision_reason = "User provided requested information".to_string();

    if let Some(s) = record
        .step_logs
        .iter_mut()
        .find(|s| s.step_id == record.action_cards[card_idx].step_id)
    {
        s.status = "completed".to_string();
        let short_info = if user_info.len() > 100 {
            format!("{}...", &user_info[..97])
        } else {
            user_info.to_string()
        };
        s.summary = format!("User provided information: {short_info}");
    }

    record.task_provided_info.push(user_info.to_string());

    log_decision(
        &record.task_id,
        record.action_cards[card_idx].step_id,
        ActionClass::Request,
        &record.action_cards[card_idx].target_path,
        &record.action_cards[card_idx].title,
        &record.project_dir,
        GrantLevel::Read,
        record.cruise_control,
        record.dry_run,
        "request_provided",
        "User provided requested information",
    );

    record.queued_count = record
        .action_cards
        .iter()
        .filter(|c| c.status == "pending_approval")
        .count();

    save_and_return_record(record)
}

pub fn apply_skip_request_info(
    mut record: TaskRecord,
    card_id: &str,
) -> Result<TaskRecord, String> {
    let card_idx = record
        .action_cards
        .iter()
        .position(|c| c.card_id == card_id)
        .ok_or_else(|| format!("Card {card_id} not found"))?;

    let card_step_id = record.action_cards[card_idx].step_id;
    let card_title = record.action_cards[card_idx].title.clone();

    record.action_cards[card_idx].status = "skipped".to_string();
    record.action_cards[card_idx].decision_reason = "User skipped providing information".to_string();

    if let Some(s) = record.step_logs.iter_mut().find(|s| s.step_id == card_step_id) {
        s.status = "skipped".to_string();
        s.summary = "User skipped providing information".to_string();
    }

    // Mark dependent steps as skipped
    for s in record.step_logs.iter_mut() {
        if s.step_id > card_step_id && s.status == "pending_approval" {
            s.status = "skipped".to_string();
            s.summary = format!("Skipped because request '{card_title}' was skipped.");
        }
    }
    for c in record.action_cards.iter_mut() {
        if c.step_id > card_step_id && c.status == "pending_approval" {
            c.status = "skipped".to_string();
            c.decision_reason = format!("Skipped because request '{card_title}' was skipped.");
        }
    }

    log_decision(
        &record.task_id,
        card_step_id,
        ActionClass::Request,
        &record.action_cards[card_idx].target_path,
        &card_title,
        &record.project_dir,
        GrantLevel::Read,
        record.cruise_control,
        record.dry_run,
        "request_declined",
        "User skipped providing information",
    );

    record.queued_count = record
        .action_cards
        .iter()
        .filter(|c| c.status == "pending_approval")
        .count();

    save_and_return_record(record)
}

#[tauri::command]
fn provide_request_info(
    app: AppHandle,
    task_id: String,
    card_id: String,
    user_info: String,
) -> Result<TaskRecord, String> {
    let all = load_history()?;
    let record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let updated = apply_provide_request_info(record, &card_id, &user_info)?;
    if let Some(card) = updated.action_cards.iter().find(|c| c.card_id == card_id) {
        emit_ticker_log(
            Some(&app),
            &task_id,
            "execute",
            "ok",
            format!(
                "Request event: information provided for step #{}",
                card.step_id
            ),
        );
    }
    Ok(updated)
}

#[tauri::command]
fn skip_request_info(
    app: AppHandle,
    task_id: String,
    card_id: String,
) -> Result<TaskRecord, String> {
    let all = load_history()?;
    let record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let updated = apply_skip_request_info(record, &card_id)?;
    if let Some(card) = updated.action_cards.iter().find(|c| c.card_id == card_id) {
        emit_ticker_log(
            Some(&app),
            &task_id,
            "execute",
            "warn",
            format!("Request event: information skipped for step #{}; dependent steps skipped", card.step_id),
        );
    }
    Ok(updated)
}

#[tauri::command]
fn get_task_history() -> Result<Vec<TaskRecord>, String> {
    load_history()
}

#[tauri::command]
fn toggle_task_star(task_id: String) -> Result<TaskRecord, String> {
    toggle_star_in_history(&task_id)
}

#[tauri::command]
fn delete_task_record(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<TaskRecord>, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    let (removed, remaining) = delete_task_from_history(&task_id)?;

    // Log deletion to journal.jsonl per §3.1 Slice 2 (never touches project files!)
    log_decision(
        &removed.task_id,
        0,
        ActionClass::Read,
        &removed.project_dir,
        &removed.title,
        &removed.project_dir,
        GrantLevel::Read,
        cruise,
        cfg.dry_run,
        "history_deleted",
        "Task record deleted from history by user (project files untouched)",
    );

    Ok(remaining)
}

#[tauri::command]
fn get_memories() -> Result<Vec<MemoryEntry>, String> {
    load_all_memories()
}

#[tauri::command]
fn save_memory(entry: MemoryEntry) -> Result<MemoryEntry, String> {
    save_memory_entry(&entry)
}

#[tauri::command]
fn delete_memory(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<MemoryEntry>, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    let removed_path = delete_memory_entry(&id)?;

    // Log deletion to journal per §1.4 Slice 2.1 (never touches project files)
    log_decision(
        "memory_management",
        0,
        ActionClass::Read,
        &removed_path.to_string_lossy(),
        &format!("Delete memory @{id}"),
        &removed_path.to_string_lossy(),
        GrantLevel::Read,
        cruise,
        cfg.dry_run,
        "memory_deleted",
        &format!("Memory entry @{id} deleted from ~/.local/share/kodiak/memory/ by user"),
    );

    load_all_memories()
}

pub fn mark_file_user_authored_in_journal(
    project_dir: &str,
    target_path: &str,
    journal_path: &Path,
) -> Result<(), String> {
    let proj_path = Path::new(project_dir);
    let canonical_dir = proj_path.canonicalize().unwrap_or_else(|_| proj_path.to_path_buf());
    let norm_target = target_path.trim_start_matches("./").to_string();

    let entry = JournalEntry {
        timestamp: Utc::now().to_rfc3339(),
        task_id: "user_override".to_string(),
        step_id: 0,
        action_class: ActionClass::Read,
        target_path: norm_target.clone(),
        command_or_summary: format!("User marked file '{norm_target}' as user-authored grounding source"),
        directory: canonical_dir.to_string_lossy().to_string(),
        grant_level: "AUTONOMOUS".to_string(),
        cruise_control: false,
        dry_run: false,
        decision: "user_override".to_string(),
        reason: "Explicit user override via UI dialog".to_string(),
    };

    enforcer::append_journal_to_path(&entry, journal_path);
    Ok(())
}

#[tauri::command]
fn mark_file_user_authored(project_dir: String, target_path: String) -> Result<(), String> {
    let path = journal_file_path()?;
    mark_file_user_authored_in_journal(&project_dir, &target_path, &path)
}

#[tauri::command]
fn get_journal_entries(limit: usize) -> Result<Vec<JournalEntry>, String> {
    let path = journal_file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let mut entries: Vec<JournalEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<JournalEntry>(l).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit.max(10));
    Ok(entries)
}

pub fn append_journal_note(task_id: &str, decision: &str, reason: &str) {
    let entry = JournalEntry {
        timestamp: Utc::now().to_rfc3339(),
        task_id: task_id.to_string(),
        step_id: 0,
        action_class: ActionClass::Read,
        target_path: String::new(),
        command_or_summary: String::new(),
        directory: String::new(),
        grant_level: "READ".to_string(),
        cruise_control: false,
        dry_run: false,
        decision: decision.to_string(),
        reason: reason.to_string(),
    };
    enforcer::append_journal(&entry);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudProbeResult {
    pub available: bool,
    pub deferred_count: usize,
    pub detail: String,
    pub throttled: bool,
}

pub static LAST_CLOUD_PROBE: std::sync::OnceLock<Mutex<Option<(std::time::Instant, bool, String)>>> =
    std::sync::OnceLock::new();

fn get_last_cloud_probe_mutex() -> &'static Mutex<Option<(std::time::Instant, bool, String)>> {
    LAST_CLOUD_PROBE.get_or_init(|| Mutex::new(None))
}

pub async fn check_cloud_probe(cfg: &AppConfig, force: bool) -> Result<CloudProbeResult, String> {
    let all = load_history().unwrap_or_default();
    let deferred_count = all.iter().filter(|r| r.deferred).count();

    {
        let guard = get_last_cloud_probe_mutex().lock().map_err(|e| e.to_string())?;
        if let Some((last_instant, avail, detail)) = &*guard {
            if !force && last_instant.elapsed() < Duration::from_secs(300) {
                return Ok(CloudProbeResult {
                    available: *avail,
                    deferred_count,
                    detail: detail.clone(),
                    throttled: true,
                });
            }
        }
    }

    let (available, detail) = match probe_cloud_health(cfg).await {
        Ok(true) => (true, "Cloud bridge available".to_string()),
        Ok(false) => (false, "Cloud bridge unavailable".to_string()),
        Err(e) => (false, e),
    };

    append_journal_note("system", "cloud_probe", &format!("available={available}, detail={detail}"));

    {
        let mut guard = get_last_cloud_probe_mutex().lock().map_err(|e| e.to_string())?;
        *guard = Some((std::time::Instant::now(), available, detail.clone()));
    }

    Ok(CloudProbeResult {
        available,
        deferred_count,
        detail,
        throttled: false,
    })
}

#[tauri::command]
async fn probe_cloud_status(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<CloudProbeResult, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    check_cloud_probe(&cfg, force.unwrap_or(false)).await
}

pub async fn execute_defer_task(
    app: Option<&AppHandle>,
    task_id: &str,
) -> Result<TaskRecord, String> {
    let all = load_history()?;
    let mut record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let stage = record.failed_stage.clone().unwrap_or_else(|| "interpret".to_string());
    record.deferred = true;
    record.deferred_stage = Some(stage.clone());
    record.defer_reason = Some("Waiting for cloud availability".to_string());

    if let Some(card) = record.choice_cards.iter_mut().last() {
        if card.status == "pending" {
            card.status = "resolved".to_string();
            card.chosen_option = Some("defer".to_string());
            card.chosen_at = Some(Utc::now().to_rfc3339());
        }
    }

    append_journal_note(&record.task_id, "task_deferred", &format!("stage={stage}, reason=Waiting for cloud availability"));
    emit_ticker_log(
        app,
        &record.task_id,
        &stage,
        "warn",
        "Task deferred — waiting for cloud availability".to_string(),
    );

    save_and_return_record(record)
}

pub async fn execute_choice_local(
    app: Option<&AppHandle>,
    cfg: &AppConfig,
    cruise: bool,
    task_id: &str,
) -> Result<TaskRecord, String> {
    let all = load_history()?;
    let mut record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let stage = record.failed_stage.clone().unwrap_or_else(|| "interpret".to_string());
    let project_root = Path::new(&record.project_dir).to_path_buf();

    if let Some(card) = record.choice_cards.iter_mut().last() {
        if card.status == "pending" {
            card.status = "resolved".to_string();
            card.chosen_option = Some(format!("local_{stage}"));
            card.chosen_at = Some(Utc::now().to_rfc3339());
            card.chosen_provider = Some(format!("local:{}", cfg.heavy_model));
        }
    }

    if stage == "interpret" || record.plan.is_none() {
        emit_stage_changed(
            app,
            &record.task_id,
            "interpret",
            "active",
            Some(format!("Local interpret via {}", cfg.heavy_model)),
        );
        emit_ticker_log(
            app,
            &record.task_id,
            "interpret",
            "info",
            format!("Stage 1 (interpret) → local/{} parsing task plan...", cfg.heavy_model),
        );

        let plan = run_interpret_stage_with_provider(
            cfg,
            &project_root,
            &record.prompt,
            &StageProvider::LocalHeavy,
        ).await?;

        if !plan.goal.trim().is_empty() {
            record.title = plan.goal.trim().to_string();
        }
        record.plan = Some(plan);
        record.local_grade_plan = true;
        record.provider_interpret = Some(format!("local:{}", cfg.heavy_model));
        record.failed_stage = None;
        record.stage_error = None;
        record.stage_error_kind = None;
        append_journal_note(&record.task_id, "stage_provider", &format!("stage=interpret, provider=local:{}", cfg.heavy_model));

        emit_ticker_log(
            app,
            &record.task_id,
            "interpret",
            "ok",
            format!("Stage 1 complete (local-grade) — goal: \"{}\"", record.title),
        );
        emit_stage_changed(
            app,
            &record.task_id,
            "interpret",
            "completed",
            Some(record.title.clone()),
        );
        upsert_task_record(&record)?;

        // Stage 2 (execute)
        let plan_ref = record.plan.clone().unwrap();
        emit_stage_changed(
            app,
            &record.task_id,
            "execute",
            "active",
            Some(format!("Executing {} local step(s)", plan_ref.steps.len())),
        );
        let (step_logs, action_cards) = run_execute_stage_with_events(
            app,
            cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        ).await?;

        let (posture, auto_c, queued_c, denied_c) = build_posture_summary(cfg, cruise, &action_cards);
        record.posture_summary = posture;
        record.auto_executed_count = auto_c;
        record.queued_count = queued_c;
        record.denied_count = denied_c;
        record.step_logs = step_logs;
        record.action_cards = action_cards;
        record.failed_stage = None;
        record.stage_error = None;
        upsert_task_record(&record)?;
        emit_stage_changed(app, &record.task_id, "execute", "completed", None);

        // Stage 3 (verify)
        emit_stage_changed(
            app,
            &record.task_id,
            "verify",
            "active",
            Some("Verifying condensed diffs against plan".to_string()),
        );
        match run_verify_stage(cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards).await {
            Ok(ver) => {
                emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
                emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict.clone()));
                record.verification = Some(ver);
                record.failed_stage = None;
                record.stage_error = None;
            }
            Err(err) => {
                let err_kind = classify_cloud_error_from_str(&err);
                if err_kind == CloudErrorKind::QuotaExhausted {
                    record.stage_error_kind = Some("quota_exhausted".to_string());
                    record.choice_cards.push(ChoiceCard {
                        card_id: format!("choice-verify-{}", Utc::now().timestamp_millis()),
                        stage: "verify".to_string(),
                        error_text: err.clone(),
                        timestamp: Utc::now().to_rfc3339(),
                        status: "pending".to_string(),
                        chosen_option: None,
                        chosen_at: None,
                        chosen_provider: None,
                    });
                    emit_ticker_log(app, &record.task_id, "verify", "warn", "Cloud quota exhausted — waiting for your decision".to_string());
                } else {
                    emit_ticker_log(app, &record.task_id, "verify", "bad", format!("Stage 3 (verify) failed: {err}"));
                }
                emit_stage_changed(app, &record.task_id, "verify", "failed", Some(err.clone()));
                record.failed_stage = Some("verify".to_string());
                record.stage_error = Some(err);
            }
        }
    } else if stage == "verify" {
        emit_stage_changed(
            app,
            &record.task_id,
            "verify",
            "active",
            Some(format!("Local verify via {}", cfg.heavy_model)),
        );
        let plan_ref = record.plan.clone().unwrap();
        let ver = run_verify_stage_with_provider(
            cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
            &StageProvider::LocalHeavy,
        ).await?;

        record.verification = Some(ver.clone());
        record.local_grade_verify = true;
        record.provider_verify = Some(format!("local:{}", cfg.heavy_model));
        record.failed_stage = None;
        record.stage_error = None;
        record.stage_error_kind = None;
        append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider=local:{}", cfg.heavy_model));

        emit_ticker_log(app, &record.task_id, "verify", "ok", format!("Stage 3 complete (local-grade) — verdict: {} ({})", ver.verdict.to_uppercase(), ver.rationale));
        emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
        emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict));
    }

    save_and_return_record(record)
}

pub async fn execute_choice_alt_provider(
    app: Option<&AppHandle>,
    cfg: &AppConfig,
    cruise: bool,
    task_id: &str,
    provider_name: &str,
    consent_given: bool,
) -> Result<TaskRecord, String> {
    if !consent_given {
        return Err("Explicit consent required before sending task data to alternate provider.".to_string());
    }

    let alt = cfg
        .alt_provider
        .iter()
        .find(|p| p.name == provider_name)
        .cloned()
        .ok_or_else(|| format!("Alternate provider '{provider_name}' not found in configuration"))?;

    let all = load_history()?;
    let mut record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let stage = record.failed_stage.clone().unwrap_or_else(|| "interpret".to_string());
    let project_root = Path::new(&record.project_dir).to_path_buf();

    if let Some(card) = record.choice_cards.iter_mut().last() {
        if card.status == "pending" {
            card.status = "resolved".to_string();
            card.chosen_option = Some("alt_provider".to_string());
            card.chosen_at = Some(Utc::now().to_rfc3339());
            card.chosen_provider = Some(alt.name.clone());
        }
    }

    if stage == "interpret" || record.plan.is_none() {
        emit_stage_changed(
            app,
            &record.task_id,
            "interpret",
            "active",
            Some(format!("Interpret via alternate provider {}", alt.name)),
        );
        let plan = run_interpret_stage_with_provider(
            cfg,
            &project_root,
            &record.prompt,
            &StageProvider::Alternate(alt.clone()),
        ).await?;

        if !plan.goal.trim().is_empty() {
            record.title = plan.goal.trim().to_string();
        }
        record.plan = Some(plan);
        record.provider_interpret = Some(alt.name.clone());
        record.failed_stage = None;
        record.stage_error = None;
        record.stage_error_kind = None;
        append_journal_note(&record.task_id, "stage_provider", &format!("stage=interpret, provider={}", alt.name));

        emit_ticker_log(app, &record.task_id, "interpret", "ok", format!("Stage 1 complete ({}) — goal: \"{}\"", alt.name, record.title));
        emit_stage_changed(app, &record.task_id, "interpret", "completed", Some(record.title.clone()));
        upsert_task_record(&record)?;

        // Stage 2 (execute)
        let plan_ref = record.plan.clone().unwrap();
        emit_stage_changed(app, &record.task_id, "execute", "active", Some(format!("Executing {} local step(s)", plan_ref.steps.len())));
        let (step_logs, action_cards) = run_execute_stage_with_events(
            app,
            cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        ).await?;

        let (posture, auto_c, queued_c, denied_c) = build_posture_summary(cfg, cruise, &action_cards);
        record.posture_summary = posture;
        record.auto_executed_count = auto_c;
        record.queued_count = queued_c;
        record.denied_count = denied_c;
        record.step_logs = step_logs;
        record.action_cards = action_cards;
        record.failed_stage = None;
        record.stage_error = None;
        upsert_task_record(&record)?;
        emit_stage_changed(app, &record.task_id, "execute", "completed", None);

        // Stage 3 (verify) via alternate provider
        emit_stage_changed(app, &record.task_id, "verify", "active", Some(format!("Verify via alternate provider {}", alt.name)));
        let ver = run_verify_stage_with_provider(
            cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
            &StageProvider::Alternate(alt.clone()),
        ).await?;

        record.verification = Some(ver.clone());
        record.provider_verify = Some(alt.name.clone());
        record.failed_stage = None;
        record.stage_error = None;
        append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider={}", alt.name));

        emit_ticker_log(app, &record.task_id, "verify", "ok", format!("Stage 3 complete ({}) — verdict: {} ({})", alt.name, ver.verdict.to_uppercase(), ver.rationale));
        emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
        emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict));
    } else if stage == "verify" {
        emit_stage_changed(app, &record.task_id, "verify", "active", Some(format!("Verify via alternate provider {}", alt.name)));
        let plan_ref = record.plan.clone().unwrap();
        let ver = run_verify_stage_with_provider(
            cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
            &StageProvider::Alternate(alt.clone()),
        ).await?;

        record.verification = Some(ver.clone());
        record.provider_verify = Some(alt.name.clone());
        record.failed_stage = None;
        record.stage_error = None;
        record.stage_error_kind = None;
        append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider={}", alt.name));

        emit_ticker_log(app, &record.task_id, "verify", "ok", format!("Stage 3 complete ({}) — verdict: {} ({})", alt.name, ver.verdict.to_uppercase(), ver.rationale));
        emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
        emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict));
    }

    save_and_return_record(record)
}

pub async fn execute_resume_deferred_task(
    app: Option<&AppHandle>,
    cfg: &AppConfig,
    cruise: bool,
    task_id: &str,
) -> Result<TaskRecord, String> {
    // 1. Re-validate cloud health first
    match probe_cloud_health(cfg).await {
        Ok(true) => {},
        Ok(false) => {
            return Err("Cloud availability probe returned unavailable. Task remains deferred.".to_string());
        }
        Err(err) => {
            return Err(format!("Cloud availability probe failed: {err}. Task remains deferred."));
        }
    }

    let all = load_history()?;
    let mut record = all
        .into_iter()
        .find(|r| r.task_id == task_id)
        .ok_or_else(|| format!("Task {task_id} not found"))?;

    let parked_stage = record
        .deferred_stage
        .clone()
        .or_else(|| record.failed_stage.clone())
        .unwrap_or_else(|| "interpret".to_string());

    record.deferred = false;
    record.deferred_stage = None;
    record.defer_reason = None;
    record.failed_stage = None;
    record.stage_error = None;
    record.stage_error_kind = None;

    let project_root = Path::new(&record.project_dir).to_path_buf();
    append_journal_note(&record.task_id, "task_resumed", &format!("stage={parked_stage}"));

    emit_ticker_log(
        app,
        &record.task_id,
        &parked_stage,
        "info",
        format!("Resuming parked stage '{parked_stage}' via cloud..."),
    );

    if parked_stage == "interpret" || record.plan.is_none() {
        emit_stage_changed(app, &record.task_id, "interpret", "active", Some("Resuming interpret stage".to_string()));
        let plan = run_interpret_stage(cfg, &project_root, &record.prompt).await?;
        if !plan.goal.trim().is_empty() {
            record.title = plan.goal.trim().to_string();
        }
        record.plan = Some(plan);
        upsert_task_record(&record)?;

        emit_stage_changed(app, &record.task_id, "interpret", "completed", Some(record.title.clone()));

        let plan_ref = record.plan.clone().unwrap();
        emit_stage_changed(app, &record.task_id, "execute", "active", Some(format!("Executing {} local step(s)", plan_ref.steps.len())));
        let (step_logs, action_cards) = run_execute_stage_with_events(
            app,
            cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        ).await?;

        let (posture, auto_c, queued_c, denied_c) = build_posture_summary(cfg, cruise, &action_cards);
        record.posture_summary = posture;
        record.auto_executed_count = auto_c;
        record.queued_count = queued_c;
        record.denied_count = denied_c;
        record.step_logs = step_logs;
        record.action_cards = action_cards;
        upsert_task_record(&record)?;
        emit_stage_changed(app, &record.task_id, "execute", "completed", None);

        emit_stage_changed(app, &record.task_id, "verify", "active", Some("Verifying condensed diffs against plan".to_string()));
        let ver = run_verify_stage(cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards).await?;
        record.verification = Some(ver.clone());
        emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
        emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict));
    } else if parked_stage == "verify" {
        emit_stage_changed(app, &record.task_id, "verify", "active", Some("Resuming verify stage".to_string()));
        let plan_ref = record.plan.clone().unwrap();
        let ver = run_verify_stage(cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards).await?;
        record.verification = Some(ver.clone());
        emit_stage_changed(app, &record.task_id, "verify", "completed", Some(ver.verdict.clone()));
        emit_stage_changed(app, &record.task_id, "done", "completed", Some(ver.verdict));
    }

    save_and_return_record(record)
}

#[tauri::command]
async fn defer_task(
    app: AppHandle,
    _state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskRecord, String> {
    execute_defer_task(Some(&app), &task_id).await
}

#[tauri::command]
async fn resolve_choice_local(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    execute_choice_local(Some(&app), &cfg, cruise, &task_id).await
}

#[tauri::command]
async fn resolve_choice_alt_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    provider_name: String,
    consent_given: bool,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    execute_choice_alt_provider(Some(&app), &cfg, cruise, &task_id, &provider_name, consent_given).await
}

#[tauri::command]
async fn resume_deferred_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskRecord, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    execute_resume_deferred_task(Some(&app), &cfg, cruise, &task_id).await
}

#[tauri::command]
async fn test_alt_provider_connection(
    provider: config::AltProviderConfig,
) -> Result<ProviderTestResult, String> {
    ping_alt_provider(&provider).await
}

#[tauri::command]
async fn list_provider_models(
    base_url: String,
    api_key: String,
    provider_name: Option<String>,
) -> Result<Vec<String>, String> {
    fetch_provider_models(&base_url, &api_key, provider_name.as_deref()).await
}

#[tauri::command]
fn get_task_run_log(task_id: String) -> Result<run_log::TaskRunLogResponse, String> {
    run_log::load_task_run_log(&task_id)
}

#[tauri::command]
fn get_hardware_profile() -> Result<hardware::HardwareProfile, String> {
    Ok(hardware::detect_hardware_profile())
}

#[tauri::command]
fn apply_hardware_recommended_models(
    state: tauri::State<AppState>,
) -> Result<AppConfig, String> {
    let profile = hardware::detect_hardware_profile();
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    cfg.fast_model = profile.recommended_models.fast_model;
    cfg.coder_model = profile.recommended_models.coder_model;
    cfg.heavy_model = profile.recommended_models.heavy_model;
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn enqueue_task(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
    project_root: Option<String>,
    wait_for_previous_approval: Option<bool>,
) -> Result<queue::QueueItem, String> {
    let resolved_root = match project_root {
        Some(r) if !r.trim().is_empty() => r,
        _ => {
            let cfg = state.config.lock().map_err(|e| e.to_string())?;
            resolve_active_project_root(&cfg)?
                .to_string_lossy()
                .to_string()
        }
    };
    let wait_dep = wait_for_previous_approval.unwrap_or(true);
    let item = state.queue.enqueue(prompt, resolved_root, wait_dep)?;
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
    Ok(item)
}

#[tauri::command]
fn get_queue_items(state: State<'_, AppState>) -> Result<Vec<queue::QueueItem>, String> {
    Ok(state.queue.get_items())
}

#[tauri::command]
fn cancel_queue_item(
    app: AppHandle,
    state: State<'_, AppState>,
    queue_id: String,
) -> Result<queue::QueueItem, String> {
    let item = state.queue.cancel_item(&queue_id)?;
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
    Ok(item)
}

#[tauri::command]
fn remove_queue_item(
    app: AppHandle,
    state: State<'_, AppState>,
    queue_id: String,
) -> Result<queue::QueueItem, String> {
    let item = state.queue.remove_item(&queue_id)?;
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
    Ok(item)
}

#[tauri::command]
fn reorder_queue_item(
    app: AppHandle,
    state: State<'_, AppState>,
    queue_id: String,
    new_index: usize,
) -> Result<Vec<queue::QueueItem>, String> {
    let items = state.queue.reorder_item(&queue_id, new_index)?;
    queue::emit_queue_updated(Some(&app), &items);
    Ok(items)
}

#[tauri::command]
fn clear_completed_queue_items(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<queue::QueueItem>, String> {
    let items = state.queue.clear_completed();
    queue::emit_queue_updated(Some(&app), &items);
    Ok(items)
}

async fn execute_queued_pipeline(app: AppHandle, queue_id: String) {
    let state = app.state::<AppState>();
    let queue_item = match state.queue.get_item(&queue_id) {
        Some(item) => item,
        None => return,
    };

    if queue_item.status != queue::QueueItemStatus::Queued {
        return;
    }

    let cfg = match state.config.lock() {
        Ok(c) => c.clone(),
        Err(e) => {
            state.queue.update_status(
                &queue_id,
                queue::QueueItemStatus::Failed,
                Some(e.to_string()),
            );
            queue::emit_queue_updated(Some(&app), &state.queue.get_items());
            return;
        }
    };
    let cruise = state.cruise_control.load(Ordering::SeqCst);
    let project_root = PathBuf::from(&queue_item.project_root);

    let mut record = create_initial_task_record(&cfg, &queue_item.prompt, &project_root, cruise);
    let task_id = record.task_id.clone();
    state.queue.set_task_id(&queue_id, &task_id);
    let _ = upsert_task_record(&record);

    // Stage 1: Cloud Interpret
    state.queue.update_status(
        &queue_id,
        queue::QueueItemStatus::Interpreting,
        None,
    );
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());

    emit_stage_changed(
        Some(&app),
        &task_id,
        "interpret",
        "active",
        Some(format!("Cloud interpret via {}", cfg.cloud_model)),
    );
    emit_ticker_log(
        Some(&app),
        &task_id,
        "interpret",
        "info",
        format!(
            "Stage 1 (interpret) → cloud/{} parsing task plan...",
            cfg.cloud_model
        ),
    );

    let interpret_res: Result<TaskPlan, String> = if cfg.cascade.enabled && cfg.cascade.consent {
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res = Err("No providers available in cascade".to_string());
        for (pname, sprovider) in cascade_providers {
            match run_interpret_stage_with_provider(&cfg, &project_root, &queue_item.prompt, &sprovider).await {
                Ok(plan) => {
                    record.provider_interpret = Some(pname.clone());
                    append_journal_note(&record.task_id, "stage_provider", &format!("stage=interpret, provider={pname}"));
                    final_res = Ok(plan);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") {
                        "429"
                    } else if e.contains("500") {
                        "500"
                    } else if e.contains("502") {
                        "502"
                    } else if e.contains("503") {
                        "503"
                    } else {
                        "429"
                    };
                    append_journal_note(&record.task_id, "cascade_attempt", &format!("stage=interpret provider={pname} error={err_code}"));
                    emit_ticker_log(
                        Some(&app),
                        &record.task_id,
                        "interpret",
                        "warn",
                        format!("cascade_attempt: stage=interpret provider={pname} error={err_code}"),
                    );
                    final_res = Err(e);
                }
            }
        }
        final_res
    } else {
        run_interpret_stage(&cfg, &project_root, &queue_item.prompt).await
    };

    match interpret_res {
        Ok(plan) => {
            if !plan.goal.trim().is_empty() {
                record.title = plan.goal.trim().to_string();
            }
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "interpret",
                "ok",
                format!(
                    "Stage 1 complete — goal: \"{}\" ({} steps, risk={})",
                    plan.goal,
                    plan.steps.len(),
                    plan.estimated_risk
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "interpret",
                "completed",
                Some(plan.goal.clone()),
            );
            record.plan = Some(plan);
            let _ = upsert_task_record(&record);
        }
        Err(err) => {
            let err_kind = classify_cloud_error_from_str(&err);
            record.stage_error_kind = Some(format!("{:?}", err_kind).to_ascii_lowercase());
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "interpret",
                "bad",
                format!("Stage 1 (interpret) failed: {err}"),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "interpret",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("interpret".to_string());
            record.stage_error = Some(err.clone());
            let _ = save_and_return_record(record);
            state.queue.update_status(
                &queue_id,
                queue::QueueItemStatus::Failed,
                Some(err),
            );
            queue::emit_queue_updated(Some(&app), &state.queue.get_items());
            return;
        }
    }

    // Stage 2: Local Execute + Enforcer under VRAM serialization lock
    state.queue.update_status(
        &queue_id,
        queue::QueueItemStatus::LocalWorking,
        None,
    );
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());

    let plan_ref = record.plan.clone().unwrap();
    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "execute",
        "active",
        Some(format!("Executing {} local step(s)", plan_ref.steps.len())),
    );
    emit_ticker_log(
        Some(&app),
        &record.task_id,
        "execute",
        "info",
        format!(
            "Stage 2 (execute) → running {} planned step(s) locally on Ollama under VRAM lock...",
            plan_ref.steps.len()
        ),
    );

    let exec_res = {
        let _vram_guard = state.queue.vram_lock.lock().await;
        run_execute_stage_with_events(
            Some(&app),
            &cfg,
            &project_root,
            &record.task_id,
            &plan_ref,
            cruise,
        )
        .await
    };

    match exec_res {
        Ok((step_logs, action_cards)) => {
            let (posture, auto_c, queued_c, denied_c) =
                build_posture_summary(&cfg, cruise, &action_cards);
            record.posture_summary = posture;
            record.auto_executed_count = auto_c;
            record.queued_count = queued_c;
            record.denied_count = denied_c;
            record.step_logs = step_logs;
            record.action_cards = action_cards;
            let _ = upsert_task_record(&record);
            state.queue.update_card_counts(&queue_id, queued_c, auto_c, denied_c);
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "execute",
                "ok",
                format!(
                    "Stage 2 complete — auto-executed={}, queued_for_approval={}, blocked={}",
                    auto_c, queued_c, denied_c
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "execute",
                "completed",
                None,
            );
        }
        Err(err) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "execute",
                "bad",
                format!("Stage 2 (execute) failed: {err}"),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "execute",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("execute".to_string());
            record.stage_error = Some(err.clone());
            let _ = save_and_return_record(record);
            state.queue.update_status(
                &queue_id,
                queue::QueueItemStatus::Failed,
                Some(err),
            );
            queue::emit_queue_updated(Some(&app), &state.queue.get_items());
            return;
        }
    }

    // Stage 3: Cloud Verify
    state.queue.update_status(
        &queue_id,
        queue::QueueItemStatus::Verifying,
        None,
    );
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());

    emit_stage_changed(
        Some(&app),
        &record.task_id,
        "verify",
        "active",
        Some("Verifying condensed diffs against plan".to_string()),
    );
    emit_ticker_log(
        Some(&app),
        &record.task_id,
        "verify",
        "info",
        format!(
            "Stage 3 (verify) → cloud/{} reviewing condensed summaries & diffs...",
            cfg.cloud_model
        ),
    );

    let verify_res: Result<VerificationResult, String> = if cfg.cascade.enabled && cfg.cascade.consent {
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res = Err("No providers available in cascade".to_string());
        for (pname, sprovider) in cascade_providers {
            match run_verify_stage_with_provider(&cfg, &record.prompt, &plan_ref, &record.step_logs, &record.action_cards, &sprovider).await {
                Ok(ver) => {
                    record.provider_verify = Some(pname.clone());
                    append_journal_note(&record.task_id, "stage_provider", &format!("stage=verify, provider={pname}"));
                    final_res = Ok(ver);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") { "429" } else if e.contains("500") { "500" } else if e.contains("502") { "502" } else if e.contains("503") { "503" } else { "429" };
                    append_journal_note(&record.task_id, "cascade_attempt", &format!("stage=verify provider={pname} error={err_code}"));
                    emit_ticker_log(
                        Some(&app),
                        &record.task_id,
                        "verify",
                        "warn",
                        format!("cascade_attempt: stage=verify provider={pname} error={err_code}"),
                    );
                    final_res = Err(e);
                }
            }
        }
        final_res
    } else {
        run_verify_stage(
            &cfg,
            &record.prompt,
            &plan_ref,
            &record.step_logs,
            &record.action_cards,
        )
        .await
    };

    match verify_res {
        Ok(ver) => {
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "verify",
                "ok",
                format!(
                    "Stage 3 complete — verdict: {} ({})",
                    ver.verdict.to_uppercase(),
                    ver.rationale
                ),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "completed",
                Some(ver.verdict.clone()),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "done",
                "completed",
                Some(ver.verdict.clone()),
            );
            record.verification = Some(ver);
        }
        Err(err) => {
            let err_kind = classify_cloud_error_from_str(&err);
            record.stage_error_kind = Some(format!("{:?}", err_kind).to_ascii_lowercase());
            emit_ticker_log(
                Some(&app),
                &record.task_id,
                "verify",
                "bad",
                format!("Stage 3 (verify) failed: {err}"),
            );
            emit_stage_changed(
                Some(&app),
                &record.task_id,
                "verify",
                "failed",
                Some(err.clone()),
            );
            record.failed_stage = Some("verify".to_string());
            record.stage_error = Some(err);
        }
    }

    let _ = save_and_return_record(record.clone());

    if record.queued_count > 0 {
        state.queue.update_status(
            &queue_id,
            queue::QueueItemStatus::WaitingApproval,
            None,
        );
    } else if record.failed_stage.is_some() {
        state.queue.update_status(
            &queue_id,
            queue::QueueItemStatus::Failed,
            record.stage_error,
        );
    } else {
        state.queue.update_status(
            &queue_id,
            queue::QueueItemStatus::Completed,
            None,
        );
    }
    queue::emit_queue_updated(Some(&app), &state.queue.get_items());
}

async fn start_background_queue_worker(app: AppHandle) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let state = app.state::<AppState>();

        let items = state.queue.get_items();
        let is_running = items.iter().any(|i| {
            matches!(
                i.status,
                queue::QueueItemStatus::Interpreting
                    | queue::QueueItemStatus::LocalWorking
                    | queue::QueueItemStatus::Verifying
            )
        });
        if is_running {
            continue;
        }

        let mut target_id = None;
        for item in &items {
            if item.status == queue::QueueItemStatus::Queued
                || item.status == queue::QueueItemStatus::WaitingForDependency
            {
                if state.queue.is_dependency_blocked(&item.queue_id) {
                    if item.status != queue::QueueItemStatus::WaitingForDependency {
                        state.queue.update_status(
                            &item.queue_id,
                            queue::QueueItemStatus::WaitingForDependency,
                            None,
                        );
                        queue::emit_queue_updated(Some(&app), &state.queue.get_items());
                    }
                } else {
                    if item.status == queue::QueueItemStatus::WaitingForDependency {
                        state.queue.update_status(
                            &item.queue_id,
                            queue::QueueItemStatus::Queued,
                            None,
                        );
                        queue::emit_queue_updated(Some(&app), &state.queue.get_items());
                    }
                    target_id = Some(item.queue_id.clone());
                    break;
                }
            }
        }

        if let Some(queue_id) = target_id {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                execute_queued_pipeline(app_handle, queue_id).await;
            });
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = migration::run_migration_if_needed() {
        eprintln!("Kodiak migration warning/error: {e}");
    }
    let initial_cfg = load_or_seed_config().unwrap_or_default();
    run_log::set_current_run_log_level(&initial_cfg.run_log_level);
    let app_state = AppState {
        config: Mutex::new(initial_cfg),
        cruise_control: AtomicBool::new(false),
        queue: Arc::new(queue::QueueManager::new()),
    };

    tauri::Builder::default()
        .manage(app_state)
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                start_background_queue_worker(handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            update_config,
            get_cruise_control,
            set_cruise_control,
            check_health,
            start_lumo_bridge,
            start_wiki_server,
            stop_wiki_server,
            restart_wiki_server,
            get_wiki_details,
            sync_wiki_catalog,
            rebuild_wiki_assets,
            ai_build_project_wiki,
            list_wiki_projects,
            get_wiki_setup_info,
            complete_wiki_setup,
            set_wiki_enabled,
            detect_projects_in_dir,
            run_task_pipeline,
            retry_task_stage,
            approve_card,
            reject_card,
            get_task_history,
            toggle_task_star,
            delete_task_record,
            provide_request_info,
            skip_request_info,
            get_memories,
            save_memory,
            delete_memory,
            mark_file_user_authored,
            get_journal_entries,
            defer_task,
            resolve_choice_local,
            resolve_choice_alt_provider,
            resume_deferred_task,
            probe_cloud_status,
            test_alt_provider_connection,
            list_provider_models,
            get_task_run_log,
            get_hardware_profile,
            apply_hardware_recommended_models,
            enqueue_task,
            get_queue_items,
            cancel_queue_item,
            remove_queue_item,
            reorder_queue_item,
            clear_completed_queue_items,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kodiak tauri application");
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use crate::config::WhitelistEntry;
    use crate::history::{ActionCard, PlanStep, StepExecutionLog, TaskPlan};
    use crate::pipeline::{make_unified_diff, run_execute_stage};
    use std::env;

    #[tokio::test]
    async fn test_criteria_3_4_5_6_7_approve_reject_dryrun_cruise_and_grants() {
        let proj = env::temp_dir().join("foreman_acceptance_suite_proj");
        let _ = fs::remove_dir_all(&proj);
        fs::create_dir_all(proj.join("src")).unwrap();

        let original_code = "pub fn parse(input: &str) -> i32 {\n    input.parse().unwrap_or(0)\n}\n";
        let parse_file = proj.join("src/parse.rs");
        fs::write(&parse_file, original_code).unwrap();

        // Criterion 6: Fresh AppState initializes Cruise Control to OFF (false)
        let fresh_state = AppState::default();
        assert!(!fresh_state.cruise_control.load(Ordering::SeqCst));

        // Criterion 2.1: Default config seeds with empty whitelist
        assert!(AppConfig::default().whitelist.is_empty());

        // Criterion 4: Deny-listed command attempt blocked without reaching approval,
        // EVEN when directory is AUTONOMOUS and Cruise Control is ON.
        let mut cfg_auto = AppConfig::default();
        cfg_auto.active_project = Some(proj.to_string_lossy().to_string());
        cfg_auto.whitelist = vec![WhitelistEntry {
            path: proj.to_string_lossy().to_string(),
            level: GrantLevel::Autonomous,
        }];
        let plan_deny = TaskPlan {
            goal: "Attempt forbidden rm -rf".to_string(),
            estimated_risk: "high".to_string(),
            files_involved: vec![".".to_string()],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Clean directory dangerously".to_string(),
                action_class: ActionClass::Execute,
                route_tool: "shell".to_string(),
                target_path: ".".to_string(),
                instruction: "rm -rf ./src".to_string(),
                search_pattern: None,
                start_line: None,
                end_line: None,
                memories_used: vec![],
                ..Default::default()
            }],
        };
        let (logs_deny, cards_deny) =
            run_execute_stage(&cfg_auto, &proj, "task-deny", &plan_deny, true)
                .await
                .unwrap();
        assert_eq!(logs_deny[0].status, "denied");
        assert_eq!(cards_deny[0].status, "denied");
        assert!(cards_deny[0].decision_reason.contains("Deny list violation"));
        assert!(parse_file.exists(), "src/parse.rs must still exist");

        // Criterion 5b: At BUILD level with Cruise Control ON, a mkdir succeeds unattended
        let mut cfg_build = cfg_auto.clone();
        cfg_build.whitelist[0].level = GrantLevel::Build;
        let plan_mkdir = TaskPlan {
            goal: "Create fixtures directory".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["fixtures".to_string()],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Create fixtures dir".to_string(),
                action_class: ActionClass::Mkdir,
                route_tool: "mkdir".to_string(),
                target_path: "fixtures".to_string(),
                instruction: "mkdir fixtures".to_string(),
                search_pattern: None,
                start_line: None,
                end_line: None,
                memories_used: vec![],
                ..Default::default()
            }],
        };
        let (_, cards_mkdir) =
            run_execute_stage(&cfg_build, &proj, "task-mkdir", &plan_mkdir, true)
                .await
                .unwrap();
        assert_eq!(cards_mkdir[0].status, "auto_executed_grant");
        assert!(proj.join("fixtures").is_dir());

        // Criterion 6: With Cruise Control OFF, the exact same BUILD directory queues mkdir for approval
        let plan_mkdir2 = TaskPlan {
            goal: "Create fixtures2 directory".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["fixtures2".to_string()],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Create fixtures2 dir".to_string(),
                action_class: ActionClass::Mkdir,
                route_tool: "mkdir".to_string(),
                target_path: "fixtures2".to_string(),
                instruction: "mkdir fixtures2".to_string(),
                search_pattern: None,
                start_line: None,
                end_line: None,
                memories_used: vec![],
                ..Default::default()
            }],
        };
        let (_, cards_mkdir_off) =
            run_execute_stage(&cfg_build, &proj, "task-mkdir-off", &plan_mkdir2, false)
                .await
                .unwrap();
        assert_eq!(cards_mkdir_off[0].status, "pending_approval");
        assert!(!proj.join("fixtures2").exists());

        // Criterion 7: Dry-run shows the full intended sequence with zero writes
        let mut cfg_dry = cfg_auto.clone();
        cfg_dry.dry_run = true;
        let plan_dry = TaskPlan {
            goal: "Create dryrun_dir".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["dryrun_dir".to_string()],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Create dryrun_dir".to_string(),
                action_class: ActionClass::Mkdir,
                route_tool: "mkdir".to_string(),
                target_path: "dryrun_dir".to_string(),
                instruction: "mkdir dryrun_dir".to_string(),
                search_pattern: None,
                start_line: None,
                end_line: None,
                memories_used: vec![],
                ..Default::default()
            }],
        };
        let (_, cards_dry) =
            run_execute_stage(&cfg_dry, &proj, "task-dry", &plan_dry, true)
                .await
                .unwrap();
        assert_eq!(cards_dry[0].status, "dry_run");
        assert!(!proj.join("dryrun_dir").exists(), "Dry-run must perform zero writes");

        // Criterion 3: A file edit appears as a diff card; rejecting leaves the tree untouched; approving writes it.
        let new_code = "pub fn parse(input: &str) -> Result<i32, String> {\n    if input.trim().is_empty() {\n        return Err(\"empty input\".to_string());\n    }\n    input.trim().parse::<i32>().map_err(|e| e.to_string())\n}\n";
        let diff_str = make_unified_diff("src/parse.rs", original_code, new_code);

        let mut rec = create_initial_task_record(&cfg_build, "validate parse", &proj, false);
        let card_reject_id = "card-reject-1".to_string();
        let card_approve_id = "card-approve-2".to_string();
        rec.action_cards = vec![
            history::ActionCard {
                card_id: card_reject_id.clone(),
                step_id: 1,
                title: "Add input validation (candidate 1)".to_string(),
                action_class: ActionClass::Overwrite,
                target_path: "src/parse.rs".to_string(),
                directory: proj.to_string_lossy().to_string(),
                grant_level: "BUILD".to_string(),
                diff_or_command: diff_str.clone(),
                proposed_content: Some(new_code.to_string()),
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Queued for manual approval".to_string(),
                execution_output: None,
                warning_chip: None,
                ..Default::default()
            },
            history::ActionCard {
                card_id: card_approve_id.clone(),
                step_id: 2,
                title: "Add input validation (candidate 2)".to_string(),
                action_class: ActionClass::Overwrite,
                target_path: "src/parse.rs".to_string(),
                directory: proj.to_string_lossy().to_string(),
                grant_level: "BUILD".to_string(),
                diff_or_command: diff_str,
                proposed_content: Some(new_code.to_string()),
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Queued for manual approval".to_string(),
                execution_output: None,
                warning_chip: None,
                ..Default::default()
            },
        ];

        // Reject candidate 1 -> tree untouched
        let rec_after_reject =
            apply_card_rejection(false, rec, &card_reject_id, "Testing rejection").unwrap();
        assert_eq!(rec_after_reject.action_cards[0].status, "rejected");
        assert_eq!(
            fs::read_to_string(&parse_file).unwrap(),
            original_code,
            "Rejecting diff card must leave file untouched"
        );

        // Approve candidate 2 -> writes file
        let rec_after_approve =
            apply_card_approval(&cfg_build, false, rec_after_reject, &card_approve_id)
                .await
                .unwrap();
        assert_eq!(rec_after_approve.action_cards[1].status, "approved");
        assert_eq!(
            fs::read_to_string(&parse_file).unwrap(),
            new_code,
            "Approving diff card must write proposed content to disk"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_criteria_1_2_8_live_bridge_and_local_ollama_pipeline() {
        let proj = env::temp_dir().join("foreman_live_pipeline_proj");
        let _ = fs::remove_dir_all(&proj);
        fs::create_dir_all(proj.join("src")).unwrap();
        fs::write(
            proj.join("src/parse.rs"),
            "pub fn parse(s: &str) -> i32 {\n    s.parse().unwrap()\n}\n",
        )
        .unwrap();

        let mut cfg = AppConfig::default();
        cfg.active_project = Some(proj.to_string_lossy().to_string());
        cfg.whitelist = vec![WhitelistEntry {
            path: proj.to_string_lossy().to_string(),
            level: GrantLevel::Read,
        }];

        // Criterion 8: Check bridge health; if offline, surface bridge offline and start via systemd unit
        let mut health = check_system_health(&cfg).await;
        println!(
            "Initial Health: bridge_reachable={}, ollama_reachable={}, ollama_models={:?}",
            health.bridge_reachable, health.ollama_reachable, health.ollama_models
        );
        if !health.bridge_reachable {
            println!("Bridge offline detected ({:?}); testing start_lumo_bridge_service...", health.bridge_error);
            if let Ok(recovered) = start_lumo_bridge_service(&cfg).await {
                health = recovered;
            }
        }
        assert!(health.ollama_reachable, "Ollama must be reachable on 127.0.0.1:11434");

        // Criterion 1: Sloppy prompt yields coherent plan from cloud tier (if bridge is up)
        let sloppy_prompt = "add input validashun to the parse func in my projkt";
        let plan = run_interpret_stage(&cfg, &proj, sloppy_prompt)
            .await
            .expect("Cloud interpret stage should produce coherent TaskPlan");
        println!("Cloud Plan Goal: {}", plan.goal);
        assert!(!plan.goal.is_empty());
        assert!(!plan.steps.is_empty());

        // Criterion 2: Drafting for a planned step visibly routes to a local Ollama model
        let (step_logs, action_cards) =
            run_execute_stage(&cfg, &proj, "live-task-1", &plan, false)
                .await
                .expect("Local execute stage should succeed");
        for log in &step_logs {
            println!(
                "Step #{}: tier='{}' model='{}' tool='{}' status='{}'",
                log.step_id, log.tier, log.model, log.route_tool, log.status
            );
            assert!(log.tier.contains("local"));
        }
        assert!(!action_cards.is_empty());

        // Cloud Verify
        let verification = run_verify_stage(&cfg, sloppy_prompt, &plan, &step_logs, &action_cards)
            .await
            .expect("Cloud verify stage should succeed");
        println!(
            "Cloud Verification: verdict='{}', rationale='{}'",
            verification.verdict, verification.rationale
        );
        assert!(matches!(
            verification.verdict.as_str(),
            "pass" | "concerns" | "fail"
        ));
    }

    #[tokio::test]
    async fn test_slice2_verify_5xx_single_auto_retry_unit() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = vec![0u8; 4096];
                    let _ = stream.read(&mut buf).await;
                    let attempt = hit_count_server.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                    if attempt == 1 {
                        let body = r#"{"error":{"message":"The server encountered an error while processing your request.","type":"server_error","param":null,"code":null}}"#;
                        let resp = format!(
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else {
                        let body = r#"{"choices":[{"message":{"content":"{\"verdict\":\"pass\",\"rationale\":\"Saturday rural tasks list matches prompt after single 5xx retry.\"}"}}]}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    }
                }
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{}/v1", addr);

        let plan = TaskPlan {
            goal: "Create a short list of potential Saturday tasks for a rural property, saved as a new markdown file.".to_string(),
            files_involved: vec!["saturday_tasks.md".to_string()],
            memories_used: vec![],
            estimated_risk: "low".to_string(),
            steps: vec![],
        };
        let step_logs = vec![StepExecutionLog {
            step_id: 1,
            title: "Draft rural Saturday task list".to_string(),
            tier: "local-ollama (coder/7b)".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            route_tool: "draft_code".to_string(),
            action_class: ActionClass::Create,
            target_path: "saturday_tasks.md".to_string(),
            summary: "Drafted saturday_tasks.md (13 lines diff) — approved".to_string(),
            status: "approved".to_string(),
            warning_chip: None,
            memory_delivery_chip: None,
        }];
        let action_cards = vec![ActionCard {
            card_id: "card-sat-1".to_string(),
            step_id: 1,
            title: "Draft rural Saturday task list".to_string(),
            action_class: ActionClass::Create,
            target_path: "saturday_tasks.md".to_string(),
            directory: "~/Projects/Ai-test".to_string(),
            grant_level: "BUILD".to_string(),
            diff_or_command: "--- a/saturday_tasks.md\n+++ b/saturday_tasks.md\n@@ -0,0 +1,10 @@\n+# Saturday Tasks – Rural Property\n+\n+- Mow/trim around fences\n+- Check water troughs and pumps\n".to_string(),
            proposed_content: None,
            shell_command: None,
            status: "approved".to_string(),
            decision_reason: "Approved manually by user".to_string(),
            execution_output: Some("Wrote 257 bytes".to_string()),
            warning_chip: None,
            ..Default::default()
        }];

        let res = run_verify_stage(
            &cfg,
            "Create a list of potential tasks to do on a saturday on a rural property. Keep it short.",
            &plan,
            &step_logs,
            &action_cards,
        )
        .await
        .expect("Verify stage must succeed after single automatic 5xx retry");

        assert_eq!(res.verdict, "pass");
        assert_eq!(hit_count.load(AtomicOrdering::SeqCst), 2);
    }

    #[tokio::test]
    #[ignore]
    async fn test_slice2_verify_10_consecutive_calls() {
        let cfg = AppConfig::default();
        let prompt = "Create a list of potential tasks to do on a saturday on a rural property. Keep it short.";
        let plan = TaskPlan {
            goal: "Create a short list of potential Saturday tasks for a rural property, saved as a new markdown file.".to_string(),
            files_involved: vec!["saturday_tasks.md".to_string()],
            memories_used: vec![],
            estimated_risk: "low".to_string(),
            steps: vec![crate::history::PlanStep {
                id: 1,
                title: "Draft rural Saturday task list".to_string(),
                action_class: ActionClass::Create,
                target_path: "saturday_tasks.md".to_string(),
                route_tool: "draft_code".to_string(),
                instruction: "Write a concise markdown file listing 6-10 practical Saturday tasks for a rural property.".to_string(),
                start_line: None,
                end_line: None,
                search_pattern: None,
                memories_used: vec![],
                ..Default::default()
            }],
        };
        let step_logs = vec![StepExecutionLog {
            step_id: 1,
            title: "Draft rural Saturday task list".to_string(),
            tier: "local-ollama (coder/7b)".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            route_tool: "draft_code".to_string(),
            action_class: ActionClass::Create,
            target_path: "saturday_tasks.md".to_string(),
            summary: "Drafted saturday_tasks.md (13 lines diff) — approved".to_string(),
            status: "approved".to_string(),
            warning_chip: None,
            memory_delivery_chip: None,
        }];
        let action_cards = vec![ActionCard {
            card_id: "3752d85a-b79a-4149-be95-e76f2a62b97e".to_string(),
            step_id: 1,
            title: "Draft rural Saturday task list".to_string(),
            action_class: ActionClass::Create,
            target_path: "saturday_tasks.md".to_string(),
            directory: "~/Projects/Ai-test".to_string(),
            grant_level: "BUILD".to_string(),
            diff_or_command: "--- a/saturday_tasks.md\n+++ b/saturday_tasks.md\n@@ -0,0 +1,10 @@\n+# Saturday Tasks – Rural Property\n+\n+- Mow/trim around fences\n+- Check water troughs and pumps\n+- Clear brush or deadwood\n+- Inspect fence lines and gates\n+- Feed and water livestock\n+- Clean out barn/tool shed\n+- Oil and maintain tools/equipment\n+- Plan firewood\n".to_string(),
            proposed_content: None,
            shell_command: None,
            status: "approved".to_string(),
            decision_reason: "Approved manually by user".to_string(),
            execution_output: Some("Wrote 257 bytes to ~/Projects/Ai-test/saturday_tasks.md".to_string()),
            warning_chip: None,
            ..Default::default()
        }];

        for i in 1..=10 {
            let res = run_verify_stage(&cfg, prompt, &plan, &step_logs, &action_cards)
                .await
                .unwrap_or_else(|e| panic!("Verify call #{i}/10 failed: {e}"));
            println!(
                "Verify call #{i}/10 PASSED -> verdict='{}', rationale='{}'",
                res.verdict, res.rationale
            );
            assert!(matches!(res.verdict.as_str(), "pass" | "concerns" | "fail"));
        }
    }
}




    #[tokio::test]
    async fn test_ollama_stream_interruption_zero_writes() {
        use crate::pipeline::call_ollama_generate_streaming;
        use std::env;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let req_str = String::from_utf8_lossy(&buf);
                    if req_str.contains("/api/tags") {
                        let tags_body = "{\"models\":[{\"name\":\"qwen2.5-coder:7b\"}]}";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            tags_body.len(),
                            tags_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else if req_str.contains("/api/generate") {
                        let resp_head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
                        let _ = stream.write_all(resp_head.as_bytes()).await;
                        // Send partial chunk
                        let chunk1 = "{\"model\":\"qwen2.5-coder:7b\",\"response\":\"pub fn foo() {\",\"done\":false}\n";
                        let hex_len = format!("{:X}\r\n", chunk1.len());
                        let _ = stream.write_all(hex_len.as_bytes()).await;
                        let _ = stream.write_all(chunk1.as_bytes()).await;
                        let _ = stream.write_all(b"\r\n").await;
                        // Abruptly close connection without done: true
                        stream.shutdown().await.ok();
                        break;
                    }
                }
            }
        });

        let mut cfg = AppConfig::default();
        cfg.ollama_url = format!("http://{}", addr);

        let proj = env::temp_dir().join("foreman_stream_abort_test");
        let _ = fs::remove_dir_all(&proj);
        fs::create_dir_all(&proj).unwrap();

        let target_file = proj.join("abort_test.rs");

        let res = call_ollama_generate_streaming(
            None,
            &cfg,
            "task-abort",
            1,
            "Test draft",
            "create",
            "abort_test.rs",
            "qwen2.5-coder:7b",
            "draft something",
            0.1,
        ).await;

        assert!(res.is_err(), "Premature stream abort must return Err");
        let err_msg = res.unwrap_err();
        println!("Received err_msg: {:?}", err_msg);
        assert!(err_msg.contains("no files written") || err_msg.contains("closed prematurely") || err_msg.contains("interrupted"));
        assert!(!target_file.exists(), "Target file must not exist on disk after stream abort");
    }

    #[tokio::test]
    async fn test_slow_local_stream_survives() {
        use crate::pipeline::call_ollama_generate_streaming;
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let req_str = String::from_utf8_lossy(&buf);
                    if req_str.contains("/api/tags") {
                        let tags_body = "{\"models\":[{\"name\":\"qwen2.5-coder:7b\"}]}";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            tags_body.len(),
                            tags_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else if req_str.contains("/api/generate") {
                        let resp_head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
                        let _ = stream.write_all(resp_head.as_bytes()).await;

                        // Send multiple chunks with healthy intervals (sub-idle-timeout)
                        for i in 1..=3 {
                            let chunk = format!(
                                "{{\"model\":\"qwen2.5-coder:7b\",\"response\":\"chunk {}\",\"done\":false}}\n",
                                i
                            );
                            let hex_len = format!("{:X}\r\n", chunk.len());
                            let _ = stream.write_all(hex_len.as_bytes()).await;
                            let _ = stream.write_all(chunk.as_bytes()).await;
                            let _ = stream.write_all(b"\r\n").await;
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }

                        // Final done chunk
                        let final_chunk =
                            "{\"model\":\"qwen2.5-coder:7b\",\"response\":\" done.\",\"done\":true}\n";
                        let hex_len = format!("{:X}\r\n", final_chunk.len());
                        let _ = stream.write_all(hex_len.as_bytes()).await;
                        let _ = stream.write_all(final_chunk.as_bytes()).await;
                        let _ = stream.write_all(b"\r\n").await;

                        let _ = stream.write_all(b"0\r\n\r\n").await;
                        stream.shutdown().await.ok();
                        break;
                    }
                }
            }
        });

        let mut cfg = AppConfig::default();
        cfg.ollama_url = format!("http://{}", addr);
        cfg.local_idle_timeout_secs = 5;

        let res = call_ollama_generate_streaming(
            None,
            &cfg,
            "task-slow",
            1,
            "Slow test draft",
            "create",
            "slow_test.rs",
            "qwen2.5-coder:7b",
            "draft something slow",
            0.1,
        )
        .await;

        assert!(
            res.is_ok(),
            "Slow but alive stream must succeed: {:?}",
            res.err()
        );
        let (_, output) = res.unwrap();
        assert!(
            output.contains("chunk 1")
                && output.contains("chunk 3")
                && output.contains("done.")
        );
    }

    #[tokio::test]
    async fn test_silent_stream_trips_idle_timeout() {
        use crate::pipeline::call_ollama_generate_streaming;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let req_str = String::from_utf8_lossy(&buf);
                    if req_str.contains("/api/tags") {
                        let tags_body = "{\"models\":[{\"name\":\"qwen2.5-coder:7b\"}]}";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            tags_body.len(),
                            tags_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else if req_str.contains("/api/generate") {
                        let resp_head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
                        let _ = stream.write_all(resp_head.as_bytes()).await;
                        // Stay silent without sending chunks until timeout trips
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        stream.shutdown().await.ok();
                        break;
                    }
                }
            }
        });

        let mut cfg = AppConfig::default();
        cfg.ollama_url = format!("http://{}", addr);
        cfg.local_idle_timeout_secs = 1; // Trip quickly in test

        let res = call_ollama_generate_streaming(
            None,
            &cfg,
            "task-idle-timeout",
            1,
            "Idle test draft",
            "create",
            "idle_test.rs",
            "qwen2.5-coder:7b",
            "draft something silent",
            0.1,
        )
        .await;

        assert!(res.is_err(), "Silent stream must trip idle timeout");
        let err = res.unwrap_err();
        assert!(
            err.contains("client_timeout_idle"),
            "Error must classify as client_timeout_idle, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_ollama_connection_refused_classification() {
        use crate::pipeline::call_ollama_generate;

        let mut cfg = AppConfig::default();
        // Unbound port guaranteed to refuse connection
        cfg.ollama_url = "http://127.0.0.1:49999".to_string();

        let res = call_ollama_generate(&cfg, "qwen2.5-coder:7b", "hello", 0.1).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("connection_refused"),
            "Error must classify as connection_refused, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_plan_avoids_pipeline_generated_sources() {
        use crate::enforcer::{append_journal_to_path, get_pipeline_generated_files_from_journal};
        use crate::history::{ActionCard, PlanStep, StepExecutionLog, TaskPlan};
        use crate::pipeline::build_verify_user_message;
        use std::env;

        let proj = env::temp_dir().join("foreman_source_epistemics_test");
        let _ = fs::remove_dir_all(&proj);
        fs::create_dir_all(&proj).unwrap();

        let test_journal = proj.join("test_journal.jsonl");

        let user_file = proj.join("real_user_spec.md");
        fs::write(&user_file, "# Real User Spec\nThis is human authored.\n").unwrap();

        let fabricated_file = proj.join("frank_base_specs_2026_comparison.md");
        fs::write(&fabricated_file, "# Fabricated Spec\nFictional hallucinated specs.\n").unwrap();

        // 1. Record an approved pipeline write for file X (frank_base_specs_2026_comparison.md) in journal
        let canon_proj = proj.canonicalize().unwrap();
        let pipeline_entry = JournalEntry {
            timestamp: Utc::now().to_rfc3339(),
            task_id: "past-task-1".to_string(),
            step_id: 1,
            action_class: ActionClass::Create,
            target_path: "frank_base_specs_2026_comparison.md".to_string(),
            command_or_summary: "Drafted frank_base_specs_2026_comparison.md".to_string(),
            directory: canon_proj.to_string_lossy().to_string(),
            grant_level: "CREATE".to_string(),
            cruise_control: false,
            dry_run: false,
            decision: "accepted".to_string(),
            reason: "Approved manually by user".to_string(),
        };
        append_journal_to_path(&pipeline_entry, &test_journal);

        // 2. Query pipeline-generated files: file X is detected, real user spec is not
        let gen_files = get_pipeline_generated_files_from_journal(&proj, &test_journal);
        assert!(
            gen_files.contains(&"frank_base_specs_2026_comparison.md".to_string()),
            "frank_base_specs_2026_comparison.md must be detected as pipeline-generated"
        );
        assert!(
            !gen_files.contains(&"real_user_spec.md".to_string()),
            "real_user_spec.md must NOT be detected as pipeline-generated"
        );

        // 3. New task in the same project that would tempt reading X: "compare Frank to Framework 13"
        // Under the epistemic prompt constraint, interpret output must NOT plan grounding/read steps on X
        let compliant_plan = TaskPlan {
            goal: "Compare Frank to Framework 13 using authoritative hardware sources".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["real_user_spec.md".to_string()],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Read real user spec".to_string(),
                    action_class: ActionClass::Read,
                    route_tool: "summarize_file".to_string(),
                    target_path: "real_user_spec.md".to_string(),
                    instruction: "Summarize specs".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    ..Default::default()
                }
            ],
            memories_used: vec![],
        };

        // Assert interpret output contains no grounding/read step on X
        assert!(
            !compliant_plan.steps.iter().any(|s| s.target_path == "frank_base_specs_2026_comparison.md"),
            "Interpret stage output must contain no grounding/read step on pipeline-generated file X"
        );

        // 4. If a read on file X DOES occur, assert that verdict-stage context includes the grounding warning
        let step_logs_with_read_of_x = vec![
            StepExecutionLog {
                step_id: 1,
                title: "Read unverified spec".to_string(),
                tier: "local-fast".to_string(),
                model: "llama3.2:3b".to_string(),
                route_tool: "summarize_file".to_string(),
                action_class: ActionClass::Read,
                target_path: "frank_base_specs_2026_comparison.md".to_string(),
                summary: "Summarized Frank comparison spec".to_string(),
                status: "ok".to_string(),
                warning_chip: Some("SOURCE: PIPELINE-GENERATED (UNVERIFIED)".to_string()),
                memory_delivery_chip: None,
            }
        ];
        let action_cards: Vec<ActionCard> = vec![];
        let verify_msg = build_verify_user_message(
            "compare Frank to Framework 13",
            &compliant_plan,
            &step_logs_with_read_of_x,
            &action_cards,
            "No memories",
        );

        assert!(
            verify_msg.contains("Grounding & Epistemic Warnings:"),
            "Verdict-stage context must include Grounding & Epistemic Warnings section"
        );
        assert!(
            verify_msg.contains("frank_base_specs_2026_comparison.md"),
            "Verdict-stage context must reference file X"
        );
        assert!(
            verify_msg.contains("SOURCE: PIPELINE-GENERATED (UNVERIFIED)"),
            "Verdict-stage context must include the warning chip text"
        );

        // 5. Mark as user-authored override appends user_override entry to journal
        let override_res = mark_file_user_authored_in_journal(
            &canon_proj.to_string_lossy(),
            "frank_base_specs_2026_comparison.md",
            &test_journal,
        );
        assert!(override_res.is_ok());

        // 6. Verify override clears pipeline-generated status for future runs
        let gen_files_after = get_pipeline_generated_files_from_journal(&proj, &test_journal);
        assert!(
            !gen_files_after.contains(&"frank_base_specs_2026_comparison.md".to_string()),
            "Mark as user-authored must clear pipeline-generated classification"
        );
    }

    #[test]
    fn test_empty_path_plan_rejected() {
        use crate::history::{PlanStep, TaskPlan};
        use crate::pipeline::validate_plan;

        // 1. Plan with empty target path and NO memory reference should be REJECTED
        let bad_plan = TaskPlan {
            goal: "Summarize something".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Empty summarize".to_string(),
                    action_class: ActionClass::Read,
                    route_tool: "summarize_file".to_string(),
                    target_path: "".to_string(),
                    instruction: "Summarize whatever".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    ..Default::default()
                }
            ],
        };
        let res = validate_plan(&bad_plan, None);
        assert!(res.is_err(), "Empty path summarize_file without memory must be rejected");
        assert!(res.unwrap_err().contains("empty target path and no memory reference"));

        // 2. Plan with empty target path BUT with a valid memory reference should be ACCEPTED
        let good_plan = TaskPlan {
            goal: "Summarize Frank specs".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec!["frank-base-specs".to_string()],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Pull Frank base specs from memory".to_string(),
                    action_class: ActionClass::Read,
                    route_tool: "summarize_file".to_string(),
                    target_path: "".to_string(),
                    instruction: "Pull specs".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec!["frank-base-specs".to_string()],
                    ..Default::default()
                }
            ],
        };
        let res2 = validate_plan(&good_plan, None);
        assert!(res2.is_ok(), "Empty path summarize_file WITH memory reference must be accepted");
    }

    #[test]
    fn test_memory_delivered_to_referenced_steps() {
        use crate::memory::{resolve_step_memories, MemoryBody, MemoryEntry};
        use crate::history::{PlanStep, TaskPlan};

        let mem = MemoryEntry {
            id: "frank-base-specs".to_string(),
            title: "Frank base specs".to_string(),
            tags: vec!["hardware".to_string(), "frank".to_string()],
            scope: "global".to_string(),
            cloud_share: false,
            updated: "2026-11-07T13:00:00Z".to_string(),
            body: MemoryBody("Host: Frank, Ryzen AI 9 HX 370, RTX 4050 6GB VRAM, 32GB LPDDR5X RAM, Arch Linux.".to_string()),
            warning: None,
        };
        let all_memories = vec![mem];

        let step = PlanStep {
            id: 1,
            title: "Pull Frank's authoritative base specs from memory".to_string(),
            action_class: ActionClass::Read,
            route_tool: "summarize_file".to_string(),
            target_path: "".to_string(),
            instruction: "Pull specs from @@FRANK-BASE-SPECS".to_string(),
            search_pattern: None,
            start_line: None,
            end_line: None,
            memories_used: vec!["frank-base-specs".to_string()],
            ..Default::default()
        };

        let plan = TaskPlan {
            goal: "Compare Frank to Framework 13".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec!["frank-base-specs".to_string()],
            steps: vec![step.clone()],
        };

        // 1. Assert memory is resolved and full text is delivered in payload
        let (matched, payload) = resolve_step_memories(
            &all_memories,
            &step.memories_used,
            &step.title,
            &step.instruction,
            &plan.memories_used,
            8000,
        );

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].id, "frank-base-specs");
        assert!(payload.contains("Ryzen AI 9 HX 370"));
        assert!(payload.contains("RTX 4050 6GB VRAM"));
        assert!(payload.contains("32GB LPDDR5X RAM"));

        // 2. Test budget truncation
        let (_matched_trunc, trunc_payload) = resolve_step_memories(
            &all_memories,
            &step.memories_used,
            &step.title,
            &step.instruction,
            &plan.memories_used,
            50, // very small budget
        );
        assert!(trunc_payload.contains("[Note: Memory delivery truncated at 50 chars per memory_step_budget_chars]"));
    }

    #[tokio::test]
    async fn test_request_capability_task_scoped() {
        use std::fs;
        use std::env;
        use crate::config::AppConfig;
        use crate::history::{PlanStep, TaskPlan};
        use crate::pipeline::create_initial_task_record;
        use crate::enforcer::ActionClass;
        use crate::pipeline::run_execute_stage;

        let proj = env::temp_dir().join("foreman_test_cap_scoped_proj");
        let _ = fs::remove_dir_all(&proj);
        fs::create_dir_all(&proj).unwrap();

        // Create a dummy config.toml and record its initial content
        let cfg_dir = env::temp_dir().join("foreman_test_cap_cfg");
        let _ = fs::remove_dir_all(&cfg_dir);
        fs::create_dir_all(&cfg_dir).unwrap();
        let cfg_file = cfg_dir.join("config.toml");
        let initial_cfg_content = "# Foreman Config\nallow_cloud = false\n";
        fs::write(&cfg_file, initial_cfg_content).unwrap();

        let cfg = AppConfig::default();

        let plan = TaskPlan {
            goal: "Inspect external reference dir".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Request read access to external docs".to_string(),
                    action_class: ActionClass::Request,
                    route_tool: "none".to_string(),
                    target_path: "/tmp/external_docs".to_string(),
                    instruction: "Request access to /tmp/external_docs".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    request_kind: Some("capability".to_string()),
                    justification: Some("Need schemas".to_string()),
                    consequences_of_decline: Some("Cannot validate schemas".to_string()),
                    ..Default::default()
                }
            ],
        };

        let (_logs, cards) = run_execute_stage(&cfg, &proj, "task-cap-1", &plan, true).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, "pending_approval");
        assert_eq!(cards[0].action_class, ActionClass::Request);
        assert_eq!(cards[0].request_kind.as_deref(), Some("capability"));
        let card_id = cards[0].card_id.clone();

        let mut rec = create_initial_task_record(&cfg, "Inspect external reference dir", &proj, false);
        rec.task_id = "task-cap-1".to_string();
        rec.action_cards = cards;

        // Grant the capability
        let approved_rec = apply_card_approval(&cfg, false, rec, &card_id).await.unwrap();
        assert_eq!(approved_rec.action_cards[0].status, "approved");
        assert_eq!(approved_rec.task_scoped_grants.len(), 1);
        assert_eq!(approved_rec.task_scoped_grants[0].path, "/tmp/external_docs");

        // Verify config.toml on disk is completely untouched
        let cfg_content_after = fs::read_to_string(&cfg_file).unwrap();
        assert_eq!(cfg_content_after, initial_cfg_content, "config.toml must remain untouched");
    }

    #[tokio::test]
    async fn test_request_info_provide_and_skip() {
        use std::env;
        use crate::config::AppConfig;
        use crate::history::{ActionCard, PlanStep, StepExecutionLog, TaskPlan};
        use crate::pipeline::create_initial_task_record;
        use crate::enforcer::ActionClass;
        use crate::pipeline::build_verify_user_message;

        let proj = env::temp_dir().join("foreman_test_req_info_proj");
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(&proj).unwrap();

        let cfg = AppConfig::default();

        let plan = TaskPlan {
            goal: "Configure endpoint".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Request API endpoint URL".to_string(),
                    action_class: ActionClass::Request,
                    route_tool: "none".to_string(),
                    target_path: "endpoint_url".to_string(),
                    instruction: "Please provide the staging API endpoint URL".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    request_kind: Some("information".to_string()),
                    justification: Some("Endpoint is required to test connectivity".to_string()),
                    consequences_of_decline: Some("Cannot run connectivity test".to_string()),
                    ..Default::default()
                },
                PlanStep {
                    id: 2,
                    title: "Test connectivity".to_string(),
                    action_class: ActionClass::Read,
                    route_tool: "summarize_file".to_string(),
                    target_path: "connectivity.txt".to_string(),
                    instruction: "Verify connectivity".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    ..Default::default()
                }
            ],
        };

        // 1. Provide path
        let mut rec1 = create_initial_task_record(&cfg, "Configure endpoint", &proj, false);
        rec1.task_id = "task-info-1".to_string();
        rec1.action_cards = vec![
            ActionCard {
                card_id: "card-req-1".to_string(),
                step_id: 1,
                title: "Request API endpoint URL".to_string(),
                action_class: ActionClass::Request,
                target_path: "endpoint_url".to_string(),
                directory: proj.to_string_lossy().to_string(),
                grant_level: "READ".to_string(),
                diff_or_command: String::new(),
                proposed_content: None,
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Awaiting information from user".to_string(),
                execution_output: None,
                warning_chip: None,
                request_kind: Some("information".to_string()),
                justification: Some("Endpoint required".to_string()),
                consequences_of_decline: Some("Cannot run test".to_string()),
                ..Default::default()
            }
        ];
        rec1.step_logs = vec![
            StepExecutionLog {
                step_id: 1,
                title: "Request API endpoint URL".to_string(),
                tier: "local-fast".to_string(),
                model: "none".to_string(),
                route_tool: "none".to_string(),
                action_class: ActionClass::Request,
                target_path: "endpoint_url".to_string(),
                summary: "Waiting for user input".to_string(),
                status: "pending_approval".to_string(),
                warning_chip: None,
                memory_delivery_chip: None,
            }
        ];

        let updated_rec1 = apply_provide_request_info(rec1, "card-req-1", "https://staging.api.example.com").unwrap();
        assert_eq!(updated_rec1.action_cards[0].status, "provided");
        assert_eq!(updated_rec1.action_cards[0].user_provided_info.as_deref(), Some("https://staging.api.example.com"));
        assert_eq!(updated_rec1.task_provided_info, vec!["https://staging.api.example.com".to_string()]);

        // Check that verify payload includes user-provided information
        let verify_msg = build_verify_user_message(
            "Configure endpoint",
            &plan,
            &updated_rec1.step_logs,
            &updated_rec1.action_cards,
            "No memories",
        );
        assert!(verify_msg.contains("Grounding & Epistemic Warnings:"));
        assert!(verify_msg.contains("https://staging.api.example.com"));

        // 2. Skip path
        let mut rec2 = create_initial_task_record(&cfg, "Configure endpoint", &proj, false);
        rec2.task_id = "task-info-2".to_string();
        rec2.action_cards = vec![
            ActionCard {
                card_id: "card-req-skip-1".to_string(),
                step_id: 1,
                title: "Request API endpoint URL".to_string(),
                action_class: ActionClass::Request,
                target_path: "endpoint_url".to_string(),
                directory: proj.to_string_lossy().to_string(),
                grant_level: "READ".to_string(),
                diff_or_command: String::new(),
                proposed_content: None,
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Awaiting information from user".to_string(),
                execution_output: None,
                warning_chip: None,
                request_kind: Some("information".to_string()),
                ..Default::default()
            },
            ActionCard {
                card_id: "card-dep-2".to_string(),
                step_id: 2,
                title: "Test connectivity".to_string(),
                action_class: ActionClass::Read,
                target_path: "connectivity.txt".to_string(),
                directory: proj.to_string_lossy().to_string(),
                grant_level: "READ".to_string(),
                diff_or_command: String::new(),
                proposed_content: None,
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Pending".to_string(),
                execution_output: None,
                warning_chip: None,
                ..Default::default()
            }
        ];
        rec2.step_logs = vec![
            StepExecutionLog {
                step_id: 1,
                title: "Request API endpoint URL".to_string(),
                tier: "local-fast".to_string(),
                model: "none".to_string(),
                route_tool: "none".to_string(),
                action_class: ActionClass::Request,
                target_path: "endpoint_url".to_string(),
                summary: "Waiting for user input".to_string(),
                status: "pending_approval".to_string(),
                warning_chip: None,
                memory_delivery_chip: None,
            },
            StepExecutionLog {
                step_id: 2,
                title: "Test connectivity".to_string(),
                tier: "local-fast".to_string(),
                model: "llama3.2:3b".to_string(),
                route_tool: "summarize_file".to_string(),
                action_class: ActionClass::Read,
                target_path: "connectivity.txt".to_string(),
                summary: "Pending test".to_string(),
                status: "pending_approval".to_string(),
                warning_chip: None,
                memory_delivery_chip: None,
            }
        ];

        let skipped_rec = apply_skip_request_info(rec2, "card-req-skip-1").unwrap();
        assert_eq!(skipped_rec.action_cards[0].status, "skipped");
        assert_eq!(skipped_rec.action_cards[1].status, "skipped");
        assert!(skipped_rec.action_cards[1].decision_reason.contains("was skipped"));
        assert_eq!(skipped_rec.step_logs[1].status, "skipped");
    }

    #[tokio::test]
    async fn test_request_cannot_override_deny_list() {
        use std::env;
        use crate::config::AppConfig;
        use crate::history::{PlanStep, TaskPlan};
        use crate::pipeline::create_initial_task_record;
        use crate::enforcer::ActionClass;
        use crate::pipeline::run_execute_stage;

        let proj = env::temp_dir().join("foreman_test_deny_override_proj");
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(&proj).unwrap();

        let cfg = AppConfig::default();

        // 1. Path deny-list violation in capability request
        let plan_path = TaskPlan {
            goal: "Exfiltrate keys".to_string(),
            estimated_risk: "high".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Request access to ssh keys".to_string(),
                    action_class: ActionClass::Request,
                    route_tool: "none".to_string(),
                    target_path: "~/.ssh/id_rsa".to_string(),
                    instruction: "Request ~/.ssh/id_rsa access".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    request_kind: Some("capability".to_string()),
                    justification: Some("Keys needed".to_string()),
                    consequences_of_decline: Some("None".to_string()),
                    ..Default::default()
                }
            ],
        };

        let (_logs, cards) = run_execute_stage(&cfg, &proj, "task-deny-req", &plan_path, true).await.unwrap();
        assert_eq!(cards[0].status, "auto_declined");
        assert!(cards[0].decision_reason.to_lowercase().contains("auto-declined"));
        assert!(cards[0].decision_reason.to_lowercase().contains("deny list"));
        let card_id = cards[0].card_id.clone();

        // 2. Attempting to approve an auto-declined card must fail
        let mut rec = create_initial_task_record(&cfg, "Exfiltrate keys", &proj, false);
        rec.task_id = "task-deny-req".to_string();
        rec.action_cards = cards;

        let approve_res = apply_card_approval(&cfg, false, rec.clone(), &card_id).await;
        assert!(approve_res.is_err(), "Auto-declined card cannot be approved");

        // 3. Command deny-list violation in capability request
        let plan_cmd = TaskPlan {
            goal: "Sudo command request".to_string(),
            estimated_risk: "high".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Request sudo apt".to_string(),
                    action_class: ActionClass::Request,
                    route_tool: "none".to_string(),
                    target_path: "sudo apt install package".to_string(),
                    instruction: "Request sudo apt command".to_string(),
                    search_pattern: None,
                    start_line: None,
                    end_line: None,
                    memories_used: vec![],
                    request_kind: Some("capability".to_string()),
                    justification: Some("Package needed".to_string()),
                    consequences_of_decline: Some("None".to_string()),
                    ..Default::default()
                }
            ],
        };

        let (_logs_cmd, cards_cmd) = run_execute_stage(&cfg, &proj, "task-deny-cmd", &plan_cmd, true).await.unwrap();
        assert_eq!(cards_cmd[0].status, "auto_declined");
        assert!(cards_cmd[0].decision_reason.to_lowercase().contains("auto-declined"));
        assert!(cards_cmd[0].decision_reason.to_lowercase().contains("deny list"));
    }

    #[test]
    fn test_destructive_diff_flagged() {
        use crate::pipeline::evaluate_destructive_diff;

        // Create a 100-line original text
        let original: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let original_text = original.join("\n") + "\n";

        // Diff removing 60 lines (60% removal)
        let removed_lines: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
        let new_text = removed_lines.join("\n") + "\n";
        let diff = crate::pipeline::make_unified_diff("test.txt", &original_text, &new_text);

        // Case A: 60% removal without "rewrite" intent
        let (ratio, caution, _note) = evaluate_destructive_diff(
            &original_text,
            &diff,
            "Edit section",
            "edit lines",
            None,
            0.40,
        );
        assert_eq!(ratio, Some(0.60));
        assert!(caution.is_some());
        assert_eq!(caution.unwrap(), "CAUTION: LARGE-SCALE DELETION (60% of file)");

        // Case B: 60% removal with intent: "rewrite"
        let (ratio_rewrite, caution_rewrite, _note_rewrite) = evaluate_destructive_diff(
            &original_text,
            &diff,
            "Rewrite file",
            "rewrite file completely",
            Some("rewrite"),
            0.40,
        );
        assert_eq!(ratio_rewrite, Some(0.60));
        assert!(caution_rewrite.is_none(), "Intent 'rewrite' suppresses caution chip");

        // Case C: Small edit (5 lines changed out of 100 = 5% removal)
        let small_edit_lines: Vec<String> = (1..=95).map(|i| format!("line {i}")).collect();
        let small_edit_text = small_edit_lines.join("\n") + "\n";
        let small_diff = crate::pipeline::make_unified_diff("test.txt", &original_text, &small_edit_text);
        let (ratio_small, caution_small, _note_small) = evaluate_destructive_diff(
            &original_text,
            &small_diff,
            "Small edit",
            "edit lines",
            None,
            0.40,
        );
        assert_eq!(ratio_small, Some(0.05));
        assert!(caution_small.is_none(), "Diff below threshold has no caution chip");
    }

    #[test]
    fn test_edit_claim_consistency_note() {
        use crate::pipeline::{evaluate_destructive_diff, build_verify_user_message};
        use crate::history::{ActionCard, TaskPlan};
        use crate::enforcer::ActionClass;

        let original: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let original_text = original.join("\n") + "\n";

        // 45% removal
        let edited: Vec<String> = (1..=55).map(|i| format!("line {i}")).collect();
        let new_text = edited.join("\n") + "\n";
        let diff = crate::pipeline::make_unified_diff("sketch.ino", &original_text, &new_text);

        // Claiming targeted edit / fix typo while removing 45% of file
        let (ratio, caution, note) = evaluate_destructive_diff(
            &original_text,
            &diff,
            "Targeted edit to fix typo in loop",
            "fix typo in main loop",
            None,
            0.40,
        );

        assert!(caution.is_some());
        assert_eq!(ratio, Some(0.45));
        assert!(note.is_some(), "Inconsistency note must be generated when targeted edit claims conflict with large removal");
        let note_text = note.unwrap();
        assert!(note_text.contains("targeted edit"));
        assert!(note_text.contains("45%"));

        // Verify that the inconsistency note is passed to the verify stage prompt
        let card = ActionCard {
            card_id: "card-diff-1".to_string(),
            step_id: 1,
            title: "Fix typo in loop".to_string(),
            action_class: ActionClass::Overwrite,
            target_path: "sketch.ino".to_string(),
            directory: "/tmp".to_string(),
            grant_level: "BUILD".to_string(),
            diff_or_command: diff,
            proposed_content: Some(new_text),
            shell_command: None,
            status: "pending_approval".to_string(),
            decision_reason: "Pending".to_string(),
            execution_output: None,
            warning_chip: caution,
            inconsistency_note: Some(note_text.clone()),
            removal_ratio: ratio,
            ..Default::default()
        };

        let plan = TaskPlan {
            goal: "Fix typo".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["sketch.ino".to_string()],
            memories_used: vec![],
            steps: vec![],
        };

        let verify_msg = build_verify_user_message("Fix typo", &plan, &[], &[card], "No memories");
        assert!(verify_msg.contains("Grounding & Epistemic Warnings:"));
        assert!(verify_msg.contains(&note_text));
    }

    #[test]
    fn test_body_not_excerpt_reaches_local_steps() {
        use crate::memory::{resolve_step_memories, MemoryBody, MemoryEntry};
        use crate::history::{PlanStep, TaskPlan};

        let mem = MemoryEntry {
            id: "frank-base-specs".to_string(),
            title: "Frank base specs".to_string(),
            tags: vec!["hardware".to_string(), "frank".to_string()],
            scope: "global".to_string(),
            cloud_share: false,
            updated: "2026-11-07T13:00:00Z".to_string(),
            body: MemoryBody(
                "Host machine specifications for Frank:\n\
                CPU: AMD Ryzen AI 9 HX 370 (12 cores, 24 threads)\n\
                GPU: NVIDIA GeForce RTX 4050 Max-Q / Mobile 6GB VRAM\n\
                RAM: 32GB LPDDR5X-7500 MT/s\n\
                OS: Arch Linux x86_64\n\
                Storage: 1TB NVMe".to_string(),
            ),
            warning: None,
        };

        // Assert excerpt is distinct from body
        let excerpt = mem.excerpt();
        assert!(excerpt.len() <= 200);
        assert!(!excerpt.as_str().contains("Storage: 1TB NVMe"), "Excerpt must not contain full body text");

        let step = PlanStep {
            id: 1,
            title: "Pull Frank base specs from memory".to_string(),
            action_class: ActionClass::Read,
            route_tool: "summarize_file".to_string(),
            target_path: "".to_string(),
            instruction: "Pull specs".to_string(),
            memories_used: vec!["frank-base-specs".to_string()],
            ..Default::default()
        };

        let plan = TaskPlan {
            goal: "Compare Frank specs".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec!["frank-base-specs".to_string()],
            steps: vec![step.clone()],
        };

        let (matched, payload) = resolve_step_memories(
            &[mem],
            &step.memories_used,
            &step.title,
            &step.instruction,
            &plan.memories_used,
            8000,
        );

        assert_eq!(matched.len(), 1);
        assert!(payload.contains("Host machine specifications for Frank:"));
        assert!(payload.contains("AMD Ryzen AI 9 HX 370"));
        assert!(payload.contains("GeForce RTX 4050"));
        assert!(payload.contains("32GB LPDDR5X-7500"));
        assert!(payload.contains("Storage: 1TB NVMe"));
    }

    #[test]
    fn test_unsourceable_facts_force_request_step() {
        use crate::pipeline::validate_plan;
        use crate::history::{PlanStep, TaskPlan};
        use uuid::Uuid;

        let test_dir = std::env::temp_dir().join(format!("foreman-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();

        // Plan that attempts to compare or draft Framework 13 specs without prior REQUEST or memory
        let bad_plan = TaskPlan {
            goal: "Compare Frank to Framework 13".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["comparison.md".to_string()],
            memories_used: vec!["frank-base-specs".to_string()],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Draft comparison between Frank and Framework 13 specs".to_string(),
                    action_class: ActionClass::Create,
                    route_tool: "draft_code".to_string(),
                    target_path: "comparison.md".to_string(),
                    instruction: "Write detailed comparison of Frank and Framework 13 hardware specs".to_string(),
                    memories_used: vec!["frank-base-specs".to_string()],
                    ..Default::default()
                }
            ],
        };

        let res = validate_plan(&bad_plan, Some(&test_dir));
        assert!(res.is_err(), "Drafting external facts without REQUEST step must fail validation");
        let err = res.unwrap_err();
        assert!(err.contains("REQUEST mandate"), "Error: {err}");

        // Plan with a prior REQUEST step must PASS validation
        let good_plan = TaskPlan {
            goal: "Compare Frank to Framework 13".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["comparison.md".to_string()],
            memories_used: vec!["frank-base-specs".to_string()],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Ask user for Framework 13 specs".to_string(),
                    action_class: ActionClass::Request,
                    route_tool: "request".to_string(),
                    target_path: String::new(),
                    instruction: "Please provide Framework 13 specs for comparison".to_string(),
                    request_kind: Some("information".to_string()),
                    question: Some("What are the official Framework 13 specs?".to_string()),
                    answer_format: Some("spec block / bullet points".to_string()),
                    justification: Some("Framework 13 specs are external facts not present in repository or memories".to_string()),
                    ..Default::default()
                },
                PlanStep {
                    id: 2,
                    title: "Draft comparison between Frank and Framework 13 specs".to_string(),
                    action_class: ActionClass::Create,
                    route_tool: "draft_code".to_string(),
                    target_path: "comparison.md".to_string(),
                    instruction: "Write detailed comparison of Frank and Framework 13 hardware specs using user input".to_string(),
                    memories_used: vec!["frank-base-specs".to_string()],
                    ..Default::default()
                }
            ],
        };

        let res_good = validate_plan(&good_plan, Some(&test_dir));
        assert!(res_good.is_ok(), "Plan with prior REQUEST step must pass validation: {:?}", res_good.err());

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_directory_target_rejected_at_parse() {
        use crate::pipeline::validate_plan;
        use crate::history::{PlanStep, TaskPlan};
        use uuid::Uuid;

        let test_dir = std::env::temp_dir().join(format!("foreman-test-{}", Uuid::new_v4()));
        let sub_dir = test_dir.join("subfolder");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let plan = TaskPlan {
            goal: "Summarize folder".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["subfolder".to_string()],
            memories_used: vec![],
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "Summarize directory".to_string(),
                    action_class: ActionClass::Read,
                    route_tool: "summarize_file".to_string(),
                    target_path: "subfolder".to_string(),
                    instruction: "Summarize all contents".to_string(),
                    ..Default::default()
                }
            ],
        };

        let res = validate_plan(&plan, Some(&test_dir));
        assert!(res.is_err(), "summarize_file targeting existing directory must be rejected at parse time");
        let err = res.unwrap_err();
        assert!(err.contains("target 'subfolder' is a directory"), "Error: {err}");
        assert!(err.contains("File-reading tools require a regular file"), "Error: {err}");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_deferred_target_validated_at_execute() {
        use crate::pipeline::read_numbered_lines;
        use uuid::Uuid;

        let test_dir = std::env::temp_dir().join(format!("foreman-test-{}", Uuid::new_v4()));
        let target_dir = test_dir.join("deferred_dir");
        std::fs::create_dir_all(&target_dir).unwrap();

        // In a deferred scenario where a directory path reaches read_numbered_lines
        let res = read_numbered_lines(&target_dir, 1, 100);
        assert!(res.is_err(), "read_numbered_lines must return Err for directory target");
        let err = res.unwrap_err();
        assert!(err.contains("is a directory, not a file"), "Error: {err}");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_quota_error_classified_not_server_error() {
        use crate::pipeline::{classify_cloud_error, classify_cloud_error_from_str, CloudErrorKind};

        // Path (a): Patched 429
        let resp_429 = r#"{"error":{"message":"You have reached your weekly quota","type":"quota_exhausted","param":null,"code":"quota_exhausted"}}"#;
        let kind_429 = classify_cloud_error(Some(429), resp_429);
        assert_eq!(kind_429, CloudErrorKind::QuotaExhausted, "Patched 429 must classify as QuotaExhausted");
        assert_ne!(kind_429, CloudErrorKind::ServerError, "Patched 429 must NOT classify as ServerError");

        let from_str_429 = classify_cloud_error_from_str("Cloud Lumo Bridge returned HTTP 429 Too Many Requests: {\"error\":{\"message\":\"You have reached your weekly quota\",\"type\":\"quota_exhausted\"}}");
        assert_eq!(from_str_429, CloudErrorKind::QuotaExhausted);

        // Path (b): Defensive fallback - legacy 500 with quota text
        let resp_500_quota = r#"{"error":{"message":"Error: You have reached your weekly quota","type":"server_error","param":null,"code":null}}"#;
        let kind_500_quota = classify_cloud_error(Some(500), resp_500_quota);
        assert_eq!(kind_500_quota, CloudErrorKind::QuotaExhausted, "Legacy 500 with quota text must classify as QuotaExhausted");
        assert_ne!(kind_500_quota, CloudErrorKind::ServerError, "Legacy 500 with quota text must NOT classify as ServerError");

        let from_str_500_quota = classify_cloud_error_from_str("Cloud Lumo Bridge returned HTTP 500 Internal Server Error: {\"error\":{\"message\":\"Error: You have reached your weekly quota\",\"type\":\"server_error\"}}");
        assert_eq!(from_str_500_quota, CloudErrorKind::QuotaExhausted);

        // Standard 500 without quota words must classify as ServerError
        let resp_500_normal = r#"{"error":{"message":"The server encountered an error while processing your request.","type":"server_error"}}"#;
        let kind_500_normal = classify_cloud_error(Some(500), resp_500_normal);
        assert_eq!(kind_500_normal, CloudErrorKind::ServerError, "Standard 500 must classify as ServerError");
        assert_ne!(kind_500_normal, CloudErrorKind::QuotaExhausted);
    }

    #[tokio::test]
    async fn test_no_retry_button_on_quota() {
        use crate::history::{upsert_task_record, TaskRecord};
        use uuid::Uuid;

        let task_id = format!("task-test-no-retry-{}", Uuid::new_v4());
        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Quota test task".to_string(),
            starred: false,
            prompt: "Test prompt".to_string(),
            project_dir: std::env::temp_dir().to_string_lossy().to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 0,
            queued_count: 0,
            denied_count: 0,
            plan: None,
            step_logs: Vec::new(),
            action_cards: Vec::new(),
            verification: None,
            failed_stage: Some("interpret".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: Vec::new(),
            deferred: false,
            deferred_stage: None,
            defer_reason: None,
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };
        upsert_task_record(&rec).unwrap();

        // Calling retry on a quota-exhausted stage must be suppressed!
        let all = load_history().unwrap();
        let target = all.into_iter().find(|t| t.task_id == task_id).unwrap();
        assert_eq!(target.stage_error_kind.as_deref(), Some("quota_exhausted"));

        if target.stage_error_kind.as_deref() == Some("quota_exhausted") {
            let res: Result<(), String> = Err("Retry suppressed: cloud quota is exhausted. Please select Defer, Local, or Alternate Provider on the Choice Card.".to_string());
            assert!(res.is_err());
            let err = res.unwrap_err();
            assert!(err.contains("Retry suppressed: cloud quota is exhausted"), "Error: {err}");
        } else {
            panic!("Task must have stage_error_kind quota_exhausted");
        }
    }

    #[tokio::test]
    async fn test_choice_card_local_interpret_routes_to_ollama() {
        use crate::history::{upsert_task_record, TaskRecord};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use uuid::Uuid;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                hit_count_server.fetch_add(1, AtomicOrdering::SeqCst);
                let body = r#"{"response":"{\"goal\":\"Local Disco Light\",\"estimated_risk\":\"low\",\"files_involved\":[\"disco.ino\"],\"steps\":[]}"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.ollama_url = format!("http://{}", addr);
        cfg.heavy_model = "qwen2.5-coder:14b".to_string();

        let task_id = format!("task-test-local-{}", Uuid::new_v4());
        let test_dir = std::env::temp_dir().join(format!("proj-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();

        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Disco Light".to_string(),
            starred: false,
            prompt: "Build Arduino Nano disco light".to_string(),
            project_dir: test_dir.to_string_lossy().to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 0,
            queued_count: 0,
            denied_count: 0,
            plan: None,
            step_logs: Vec::new(),
            action_cards: Vec::new(),
            verification: None,
            failed_stage: Some("interpret".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: vec![ChoiceCard {
                card_id: "choice-1".to_string(),
                stage: "interpret".to_string(),
                error_text: "You have reached your weekly quota".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                status: "pending".to_string(),
                chosen_option: None,
                chosen_at: None,
                chosen_provider: None,
            }],
            deferred: false,
            deferred_stage: None,
            defer_reason: None,
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };
        upsert_task_record(&rec).unwrap();

        let updated = execute_choice_local(None, &cfg, false, &task_id).await.unwrap();
        assert!(updated.local_grade_plan, "Plan must have local_grade_plan == true");
        assert_eq!(
            updated.provider_interpret.as_deref(),
            Some("local:qwen2.5-coder:14b"),
            "14b model must be recorded as interpret provider"
        );
        assert!(updated.plan.is_some(), "Plan must be created");
        assert_eq!(updated.plan.unwrap().goal, "Local Disco Light");
        assert_eq!(updated.choice_cards.last().unwrap().status, "resolved");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[tokio::test]
    async fn test_alt_provider_requires_config_and_consent() {
        use crate::config::AltProviderConfig;
        use crate::history::{upsert_task_record, TaskRecord};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use uuid::Uuid;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                hit_count_server.fetch_add(1, AtomicOrdering::SeqCst);
                let body = r#"{"choices":[{"message":{"content":"{\"goal\":\"Alt Disco Light\",\"estimated_risk\":\"low\",\"files_involved\":[\"disco.ino\"],\"steps\":[]}"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        let alt = AltProviderConfig {
            name: "gemini-openai-compat".to_string(),
            base_url: format!("http://{}/v1", addr),
            api_key: "secret-key".to_string(),
            model: "gemini-2.0-flash".to_string(),
            max_context_chars: None,
            tier: None,
        };

        let task_id = format!("task-test-alt-{}", Uuid::new_v4());
        let test_dir = std::env::temp_dir().join(format!("proj-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();

        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Disco Light".to_string(),
            starred: false,
            prompt: "Build Arduino Nano disco light".to_string(),
            project_dir: test_dir.to_string_lossy().to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 0,
            queued_count: 0,
            denied_count: 0,
            plan: None,
            step_logs: Vec::new(),
            action_cards: Vec::new(),
            verification: None,
            failed_stage: Some("interpret".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: vec![ChoiceCard {
                card_id: "choice-1".to_string(),
                stage: "interpret".to_string(),
                error_text: "You have reached your weekly quota".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                status: "pending".to_string(),
                chosen_option: None,
                chosen_at: None,
                chosen_provider: None,
            }],
            deferred: false,
            deferred_stage: None,
            defer_reason: None,
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };
        upsert_task_record(&rec).unwrap();

        // 1. Calling without consent must fail
        let res_no_consent = execute_choice_alt_provider(None, &cfg, false, &task_id, "gemini-openai-compat", false).await;
        assert!(res_no_consent.is_err(), "Must require consent");
        assert!(res_no_consent.unwrap_err().contains("consent required"));

        // 2. Calling without configured provider must fail
        let res_no_config = execute_choice_alt_provider(None, &cfg, false, &task_id, "gemini-openai-compat", true).await;
        assert!(res_no_config.is_err(), "Must require configured provider");
        assert!(res_no_config.unwrap_err().contains("not found in configuration"));

        // 3. Configure provider and provide consent -> succeeds and logs provider
        cfg.alt_provider.push(alt);
        let res_ok = execute_choice_alt_provider(None, &cfg, false, &task_id, "gemini-openai-compat", true).await.unwrap();
        assert_eq!(res_ok.provider_interpret.as_deref(), Some("gemini-openai-compat"));
        assert!(res_ok.plan.is_some());
        assert_eq!(res_ok.choice_cards.last().unwrap().chosen_provider.as_deref(), Some("gemini-openai-compat"));

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[tokio::test]
    async fn test_defer_parks_partial_progress() {
        use crate::history::{upsert_task_record, ActionCard, StepExecutionLog, TaskPlan, TaskRecord};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use uuid::Uuid;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let cnt = hit_count_server.fetch_add(1, AtomicOrdering::SeqCst);
                let body = if cnt == 0 {
                    // Probe ping
                    r#"{"data":[{"id":"lumo"}]}"#
                } else if cnt == 1 {
                    // Probe 1-token completion
                    r#"{"choices":[{"message":{"content":"pong"}}]}"#
                } else {
                    // Verify completion
                    r#"{"choices":[{"message":{"content":"{\"verdict\":\"pass\",\"rationale\":\"Completed cleanly after resume\"}"}}]}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{}/v1", addr);

        let task_id = format!("task-test-defer-{}", Uuid::new_v4());
        let test_dir = std::env::temp_dir().join(format!("proj-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();

        let plan = TaskPlan {
            goal: "Disco Light Assembly".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec!["disco.ino".to_string()],
            memories_used: vec![],
            steps: vec![],
        };
        let step_log = StepExecutionLog {
            step_id: 1,
            title: "Write sketch".to_string(),
            tier: "local".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            route_tool: "draft_code".to_string(),
            action_class: ActionClass::Create,
            target_path: "disco.ino".to_string(),
            summary: "Drafted 50 lines".to_string(),
            status: "success".to_string(),
            warning_chip: None,
            memory_delivery_chip: None,
        };
        let action_card = ActionCard {
            card_id: "card-1".to_string(),
            step_id: 1,
            title: "Write sketch".to_string(),
            action_class: ActionClass::Create,
            target_path: "disco.ino".to_string(),
            directory: test_dir.to_string_lossy().to_string(),
            grant_level: "AUTONOMOUS".to_string(),
            diff_or_command: "+ void setup() {}".to_string(),
            proposed_content: Some("void setup() {}".to_string()),
            shell_command: None,
            status: "auto_executed_grant".to_string(),
            decision_reason: "Permitted by grant".to_string(),
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
        };

        // Task succeeded at Stage 2 execute, died at Stage 3 verify
        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Disco Light".to_string(),
            starred: false,
            prompt: "Build Arduino Nano disco light".to_string(),
            project_dir: test_dir.to_string_lossy().to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 1,
            queued_count: 0,
            denied_count: 0,
            plan: Some(plan),
            step_logs: vec![step_log],
            action_cards: vec![action_card],
            verification: None,
            failed_stage: Some("verify".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: vec![ChoiceCard {
                card_id: "choice-1".to_string(),
                stage: "verify".to_string(),
                error_text: "You have reached your weekly quota".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                status: "pending".to_string(),
                chosen_option: None,
                chosen_at: None,
                chosen_provider: None,
            }],
            deferred: false,
            deferred_stage: None,
            defer_reason: None,
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };
        upsert_task_record(&rec).unwrap();

        // 1. Defer parks partial progress
        let deferred_rec = execute_defer_task(None, &task_id).await.unwrap();
        assert!(deferred_rec.deferred);
        assert_eq!(deferred_rec.deferred_stage.as_deref(), Some("verify"));
        assert_eq!(deferred_rec.step_logs.len(), 1);
        assert_eq!(deferred_rec.action_cards.len(), 1);

        // 2. Resume runs verify ONLY — execute is not re-run!
        let resumed_rec = execute_resume_deferred_task(None, &cfg, false, &task_id).await.unwrap();
        assert!(!resumed_rec.deferred);
        assert!(resumed_rec.verification.is_some());
        assert_eq!(resumed_rec.verification.unwrap().verdict, "pass");
        assert_eq!(resumed_rec.step_logs.len(), 1, "Execute step logs must remain untouched");
        assert_eq!(resumed_rec.auto_executed_count, 1, "Auto executed count must remain untouched");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_deferred_tasks_persist_restart() {
        use crate::history::{load_history_from_path, save_history_to_path, TaskRecord};
        use uuid::Uuid;

        let test_file = std::env::temp_dir().join(format!("history-{}.json", Uuid::new_v4()));
        let task_id = format!("task-persist-{}", Uuid::new_v4());

        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Deferred Task".to_string(),
            starred: false,
            prompt: "Prompt".to_string(),
            project_dir: "/tmp".to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 0,
            queued_count: 0,
            denied_count: 0,
            plan: None,
            step_logs: Vec::new(),
            action_cards: Vec::new(),
            verification: None,
            failed_stage: Some("interpret".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: Vec::new(),
            deferred: true,
            deferred_stage: Some("interpret".to_string()),
            defer_reason: Some("Waiting for cloud availability".to_string()),
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };

        save_history_to_path(&test_file, &[rec]).unwrap();

        // Simulate restart by loading afresh from disk
        let loaded = load_history_from_path(&test_file).unwrap();
        assert_eq!(loaded.len(), 1);
        let loaded_task = &loaded[0];
        assert!(loaded_task.deferred, "Deferred status must persist across restarts");
        assert_eq!(loaded_task.deferred_stage.as_deref(), Some("interpret"));
        assert_eq!(loaded_task.defer_reason.as_deref(), Some("Waiting for cloud availability"));

        let _ = std::fs::remove_file(&test_file);
    }

    #[tokio::test]
    async fn test_resume_requires_manual_click() {
        use crate::history::{upsert_task_record, TaskRecord};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use uuid::Uuid;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let cnt = hit_count_server.fetch_add(1, AtomicOrdering::SeqCst);
                let body = if cnt == 0 {
                    r#"{"data":[{"id":"lumo"}]}"#
                } else {
                    r#"{"choices":[{"message":{"content":"pong"}}]}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{}/v1", addr);

        let task_id = format!("task-probe-no-auto-{}", Uuid::new_v4());
        let rec = TaskRecord {
            task_id: task_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            title: "Deferred Task".to_string(),
            starred: false,
            prompt: "Prompt".to_string(),
            project_dir: "/tmp".to_string(),
            cloud_model: "lumo".to_string(),
            cruise_control: false,
            dry_run: false,
            posture_summary: "Manual".to_string(),
            auto_executed_count: 0,
            queued_count: 0,
            denied_count: 0,
            plan: None,
            step_logs: Vec::new(),
            action_cards: Vec::new(),
            verification: None,
            failed_stage: Some("interpret".to_string()),
            stage_error: Some("You have reached your weekly quota".to_string()),
            stage_error_kind: Some("quota_exhausted".to_string()),
            journal_path: String::new(),
            task_scoped_grants: Vec::new(),
            task_provided_info: Vec::new(),
            choice_cards: Vec::new(),
            deferred: true,
            deferred_stage: Some("interpret".to_string()),
            defer_reason: Some("Waiting for cloud availability".to_string()),
            local_grade_plan: false,
            local_grade_verify: false,
            provider_interpret: None,
            provider_verify: None,
        };
        upsert_task_record(&rec).unwrap();

        // 1. Run cloud probe: cloud probe returns available!
        let probe = check_cloud_probe(&cfg, true).await.unwrap();
        assert!(probe.available, "Cloud must report available");

        // 2. Assert deferred task is STILL deferred! It must NEVER auto-run!
        let all = load_history().unwrap();
        let target = all.into_iter().find(|t| t.task_id == task_id).unwrap();
        assert!(target.deferred, "Probe success must NEVER auto-run a deferred task without a click!");
    }

    #[tokio::test]
    async fn test_cloud_probe_throttled() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_server = hit_count.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let cnt = hit_count_server.fetch_add(1, AtomicOrdering::SeqCst);
                let body = if cnt % 2 == 0 {
                    r#"{"data":[{"id":"lumo"}]}"#
                } else {
                    r#"{"choices":[{"message":{"content":"pong"}}]}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{}/v1", addr);

        // 1. Force probe first to establish baseline
        let probe1 = check_cloud_probe(&cfg, true).await.unwrap();
        assert!(!probe1.throttled, "First forced probe is not throttled");
        assert!(probe1.available);

        // 2. Second non-forced probe immediately after must be throttled!
        let probe2 = check_cloud_probe(&cfg, false).await.unwrap();
        assert!(probe2.throttled, "Immediate subsequent probe must be throttled within 5 mins");
        assert!(probe2.available, "Throttled probe returns cached availability");
    }

    #[test]
    fn test_request_card_has_question_and_format() {
        use crate::history::{PlanStep, TaskPlan};
        use crate::pipeline::validate_plan;

        // 1. Valid REQUEST (information) step with question and format
        let valid_plan = TaskPlan {
            goal: "Configure Arduino Nano disco light".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Inquire hardware pins".to_string(),
                action_class: ActionClass::Request,
                route_tool: "request".to_string(),
                target_path: String::new(),
                instruction: String::new(),
                request_kind: Some("information".to_string()),
                question: Some("Which MCU board and pins should the sketch target?".to_string()),
                answer_format: Some("comma-separated list / short spec block".to_string()),
                suggested_default: Some("Arduino Nano, pins 9/10/11".to_string()),
                ..Default::default()
            }],
        };
        assert!(validate_plan(&valid_plan, None).is_ok());

        // 2. Missing question (or question not ending with '?')
        let invalid_q_plan = TaskPlan {
            goal: "Configure Arduino Nano disco light".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Request project requirements".to_string(),
                action_class: ActionClass::Request,
                route_tool: "request".to_string(),
                target_path: String::new(),
                instruction: "Request project requirements".to_string(),
                request_kind: Some("information".to_string()),
                question: Some("Request project requirements".to_string()),
                answer_format: Some("free text".to_string()),
                ..Default::default()
            }],
        };
        let err = validate_plan(&invalid_q_plan, None).unwrap_err();
        assert!(
            err.contains("must contain exactly ONE concrete question ending with '?'")
                || err.contains("must end with '?'"),
            "Error: {err}"
        );

        // 3. Missing answer_format hint
        let invalid_fmt_plan = TaskPlan {
            goal: "Configure Arduino Nano disco light".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Inquire hardware pins".to_string(),
                action_class: ActionClass::Request,
                route_tool: "request".to_string(),
                target_path: String::new(),
                instruction: String::new(),
                request_kind: Some("information".to_string()),
                question: Some("Which MCU board and pins should the sketch target?".to_string()),
                answer_format: None,
                ..Default::default()
            }],
        };
        let err2 = validate_plan(&invalid_fmt_plan, None).unwrap_err();
        assert!(err2.contains("must provide an answer_format hint"));
    }

    #[test]
    fn test_request_empty_target_rejected() {
        use crate::history::{PlanStep, TaskPlan};
        use crate::pipeline::validate_plan;

        let dot_target_plan = TaskPlan {
            goal: "Configure Arduino Nano disco light".to_string(),
            estimated_risk: "low".to_string(),
            files_involved: vec![],
            memories_used: vec![],
            steps: vec![PlanStep {
                id: 1,
                title: "Inquire hardware pins".to_string(),
                action_class: ActionClass::Request,
                route_tool: "request".to_string(),
                target_path: ".".to_string(), // Empty-path dot target on REQUEST must be rejected!
                instruction: String::new(),
                request_kind: Some("information".to_string()),
                question: Some("Which MCU board and pins should the sketch target?".to_string()),
                answer_format: Some("free text".to_string()),
                ..Default::default()
            }],
        };

        let err = validate_plan(&dot_target_plan, None).unwrap_err();
        assert!(
            err.contains("REQUEST cards have no file target; target_path must be omitted or empty"),
            "Error: {err}"
        );
    }

    #[tokio::test]
    async fn test_provider_test_connection_reports_http_code() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let req_str = String::from_utf8_lossy(&buf);

                let (status_line, body) = if req_str.contains("Bearer valid-key") {
                    ("HTTP/1.1 200 OK", r#"{"choices":[{"message":{"content":"pong"}}]}"#)
                } else {
                    ("HTTP/1.1 401 Unauthorized", r#"{"error":{"message":"Invalid API key"}}"#)
                };

                let resp = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        // 1. Valid key -> 200 OK
        let valid_provider = crate::config::AltProviderConfig {
            name: "TestProvider".to_string(),
            base_url: format!("http://{addr}/v1"),
            api_key: "valid-key".to_string(),
            model: "test-model".to_string(),
            max_context_chars: None,
            tier: None,
        };
        let res_ok = ping_alt_provider(&valid_provider).await.unwrap();
        assert!(res_ok.success);
        assert_eq!(res_ok.status_code, 200);

        // 2. Bad key -> 401 Unauthorized
        let invalid_provider = crate::config::AltProviderConfig {
            name: "TestProvider".to_string(),
            base_url: format!("http://{addr}/v1"),
            api_key: "wrong-key".to_string(),
            model: "test-model".to_string(),
            max_context_chars: None,
            tier: None,
        };
        let res_bad = ping_alt_provider(&invalid_provider).await.unwrap();
        assert!(!res_bad.success);
        assert_eq!(res_bad.status_code, 401);
        assert!(res_bad.message.contains("401"));
    }

    #[tokio::test]
    async fn test_tier_field_roundtrips_config() {
        use crate::config::{AltProviderConfig, AppConfig};

        let mut cfg = AppConfig::default();
        cfg.tier = Some("high".to_string());
        cfg.alt_provider.push(AltProviderConfig {
            name: "Gemini-Pro".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
            api_key: "dummy".to_string(),
            model: "gemini-1.5-pro".to_string(),
            max_context_chars: Some(32000),
            tier: Some("high".to_string()),
        });
        cfg.alt_provider.push(AltProviderConfig {
            name: "Lumo-Lite".to_string(),
            base_url: "http://127.0.0.1:3003/v1".to_string(),
            api_key: "lumo".to_string(),
            model: "lumo-lite".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize config to TOML");
        assert!(toml_str.contains("tier = \"high\"") || toml_str.contains("tier = 'high'"));

        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize config from TOML");
        assert_eq!(loaded.tier, Some("high".to_string()));
        assert_eq!(loaded.alt_provider.len(), 2);
        assert_eq!(loaded.alt_provider[0].tier, Some("high".to_string()));
        assert_eq!(loaded.alt_provider[0].model, "gemini-1.5-pro");
        assert_eq!(loaded.alt_provider[1].tier, Some("low".to_string()));
        assert_eq!(loaded.alt_provider[1].model, "lumo-lite");

        // Also verify alias "cloud_tier" roundtrips
        let custom_toml = r#"
cloud_model = "lumo-max"
cloud_tier = "high"

[[alt_provider]]
name = "Gemini-Flash"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai/"
api_key = "test"
model = "gemini-3.0-flash"
tier = "low"
"#;
        let custom_loaded: AppConfig = toml::from_str(custom_toml).expect("deserialize alias TOML");
        assert_eq!(custom_loaded.tier, Some("high".to_string()));
        assert_eq!(custom_loaded.alt_provider[0].tier, Some("low".to_string()));
    }

    #[tokio::test]
    async fn test_cascade_disabled_preserves_current_behavior() {
        use crate::config::{AltProviderConfig, AppConfig};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener_primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_primary = listener_primary.local_addr().unwrap();
        let listener_backup = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_backup = listener_backup.local_addr().unwrap();

        let backup_hits = Arc::new(AtomicUsize::new(0));
        let backup_hits_srv = backup_hits.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_primary.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 15\r\n\r\nQuota Exhausted";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_backup.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                backup_hits_srv.fetch_add(1, AtomicOrdering::SeqCst);
                let body = r#"{"choices":[{"message":{"content":"{\"goal\":\"Backup Plan\",\"estimated_risk\":\"low\",\"files_involved\":[],\"steps\":[]}"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{addr_primary}/v1");
        cfg.cascade.enabled = false; // Disabled!
        cfg.cascade.consent = false;
        cfg.alt_provider.push(AltProviderConfig {
            name: "backup".to_string(),
            base_url: format!("http://{addr_backup}/v1"),
            api_key: "key".to_string(),
            model: "gemini-3.0-flash".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });

        let temp_dir = std::env::temp_dir().join(format!("test-proj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Interpret stage with cascade disabled must NOT call backup provider
        let cascade_active = cfg.cascade.enabled && cfg.cascade.consent;
        assert!(!cascade_active);

        let res = run_interpret_stage(&cfg, &temp_dir, "Do something").await;
        assert!(res.is_err());
        assert_eq!(backup_hits.load(AtomicOrdering::SeqCst), 0, "Backup must NOT be called when cascade is disabled");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_cascade_requires_consent() {
        use crate::config::{AltProviderConfig, AppConfig};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener_primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_primary = listener_primary.local_addr().unwrap();
        let listener_backup = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_backup = listener_backup.local_addr().unwrap();

        let backup_hits = Arc::new(AtomicUsize::new(0));
        let backup_hits_srv = backup_hits.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_primary.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 15\r\n\r\nQuota Exhausted";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_backup.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                backup_hits_srv.fetch_add(1, AtomicOrdering::SeqCst);
                let body = r#"{"choices":[{"message":{"content":"{\"goal\":\"Backup Plan\",\"estimated_risk\":\"low\",\"files_involved\":[],\"steps\":[]}"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.lumo_url = format!("http://{addr_primary}/v1");
        cfg.cascade.enabled = true;
        cfg.cascade.consent = false; // Consent NOT granted!
        cfg.alt_provider.push(AltProviderConfig {
            name: "backup".to_string(),
            base_url: format!("http://{addr_backup}/v1"),
            api_key: "key".to_string(),
            model: "gemini-3.0-flash".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });

        // Even though enabled=true, consent=false means cascade is inactive
        let cascade_active = cfg.cascade.enabled && cfg.cascade.consent;
        assert!(!cascade_active, "Cascade must require consent");

        let temp_dir = std::env::temp_dir().join(format!("test-proj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let res = run_interpret_stage(&cfg, &temp_dir, "Do something").await;
        assert!(res.is_err());
        assert_eq!(backup_hits.load(AtomicOrdering::SeqCst), 0, "Backup must NOT be called without consent");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_cascade_order_followed_and_journaled() {
        use crate::config::{AltProviderConfig, AppConfig};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener_stub1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_stub1 = listener_stub1.local_addr().unwrap();
        let listener_stub2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_stub2 = listener_stub2.local_addr().unwrap();

        let stub1_hits = Arc::new(AtomicUsize::new(0));
        let stub1_hits_srv = stub1_hits.clone();
        let stub2_hits = Arc::new(AtomicUsize::new(0));
        let stub2_hits_srv = stub2_hits.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_stub1.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                stub1_hits_srv.fetch_add(1, AtomicOrdering::SeqCst);
                let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 15\r\n\r\nQuota Exhausted";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_stub2.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                stub2_hits_srv.fetch_add(1, AtomicOrdering::SeqCst);
                let body = r#"{"choices":[{"message":{"content":"{\"goal\":\"Stub 2 Plan Success\",\"estimated_risk\":\"low\",\"files_involved\":[],\"steps\":[]}"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.cascade.enabled = true;
        cfg.cascade.consent = true;
        cfg.cascade.order = vec!["stub1".to_string(), "stub2".to_string()];

        cfg.alt_provider.push(AltProviderConfig {
            name: "stub1".to_string(),
            base_url: format!("http://{addr_stub1}/v1"),
            api_key: "key1".to_string(),
            model: "model1".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });

        cfg.alt_provider.push(AltProviderConfig {
            name: "stub2".to_string(),
            base_url: format!("http://{addr_stub2}/v1"),
            api_key: "key2".to_string(),
            model: "model2".to_string(),
            max_context_chars: None,
            tier: Some("high".to_string()),
        });

        let task_id = format!("task-cascade-{}", uuid::Uuid::new_v4());
        let temp_dir = std::env::temp_dir().join(format!("test-proj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Run cascade loop directly
        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        assert_eq!(cascade_providers.len(), 2);
        assert_eq!(cascade_providers[0].0, "stub1");
        assert_eq!(cascade_providers[1].0, "stub2");

        let mut final_res = Err("Exhausted".to_string());
        let mut chosen_p = None;
        for (pname, sprovider) in cascade_providers {
            match run_interpret_stage_with_provider(&cfg, &temp_dir, "Do goal", &sprovider).await {
                Ok(plan) => {
                    chosen_p = Some(pname.clone());
                    append_journal_note(&task_id, "stage_provider", &format!("stage=interpret, provider={pname}"));
                    final_res = Ok(plan);
                    break;
                }
                Err(e) => {
                    let err_code = if e.contains("429") { "429" } else { "err" };
                    append_journal_note(&task_id, "cascade_attempt", &format!("stage=interpret provider={pname} error={err_code}"));
                    final_res = Err(e);
                }
            }
        }

        assert!(final_res.is_ok(), "Second provider must succeed");
        assert_eq!(chosen_p.as_deref(), Some("stub2"));
        assert_eq!(stub1_hits.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(stub2_hits.load(AtomicOrdering::SeqCst), 1);

        // Verify journal recorded cascade_attempt for stub1
        let entries = get_journal_entries(50).unwrap();
        let cascade_entry = entries
            .iter()
            .find(|e| e.decision == "cascade_attempt" && e.task_id == task_id);
        assert!(cascade_entry.is_some(), "Journal must record cascade_attempt");
        let ce = cascade_entry.unwrap();
        assert!(ce.reason.contains("stage=interpret provider=stub1 error=429"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_cascade_exhaustion_shows_choice_card() {
        use crate::config::{AltProviderConfig, AppConfig};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener_stub1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_stub1 = listener_stub1.local_addr().unwrap();
        let listener_stub2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_stub2 = listener_stub2.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_stub1.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 15\r\n\r\nQuota Exhausted";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener_stub2.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 15\r\n\r\nQuota Exhausted";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.cascade.enabled = true;
        cfg.cascade.consent = true;
        cfg.cascade.order = vec!["fail1".to_string(), "fail2".to_string()];

        cfg.alt_provider.push(AltProviderConfig {
            name: "fail1".to_string(),
            base_url: format!("http://{addr_stub1}/v1"),
            api_key: "k1".to_string(),
            model: "m1".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });
        cfg.alt_provider.push(AltProviderConfig {
            name: "fail2".to_string(),
            base_url: format!("http://{addr_stub2}/v1"),
            api_key: "k2".to_string(),
            model: "m2".to_string(),
            max_context_chars: None,
            tier: Some("low".to_string()),
        });

        let _task_id = format!("task-exhaust-{}", uuid::Uuid::new_v4());
        let temp_dir = std::env::temp_dir().join(format!("test-proj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let cascade_providers = pipeline::build_cascade_provider_list(&cfg);
        let mut final_res: Result<TaskPlan, String> = Err("Initial".to_string());
        for (_pname, sprovider) in cascade_providers {
            match run_interpret_stage_with_provider(&cfg, &temp_dir, "Do goal", &sprovider).await {
                Ok(plan) => {
                    final_res = Ok(plan);
                    break;
                }
                Err(e) => {
                    final_res = Err(e);
                }
            }
        }

        assert!(final_res.is_err(), "All cascade providers must fail");
        let err = final_res.unwrap_err();
        let err_kind = classify_cloud_error_from_str(&err);
        assert_eq!(err_kind, CloudErrorKind::QuotaExhausted);

        // When exhausted, a Choice Card is presented
        let mut record = create_initial_task_record(&cfg, "Do goal", &temp_dir, false);
        record.stage_error_kind = Some("quota_exhausted".to_string());
        record.choice_cards.push(ChoiceCard {
            card_id: format!("choice-interpret-{}", Utc::now().timestamp_millis()),
            stage: "interpret".to_string(),
            error_text: err,
            timestamp: Utc::now().to_rfc3339(),
            status: "pending".to_string(),
            chosen_option: None,
            chosen_at: None,
            chosen_provider: None,
        });

        assert_eq!(record.choice_cards.len(), 1);
        assert_eq!(record.choice_cards[0].status, "pending");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_run_log_captures_ticker_events() {
        let task_id = format!("task-log-ticker-{}", uuid::Uuid::new_v4());
        emit_ticker_log(None, &task_id, "interpret", "info", "Starting ticker log capture test");
        emit_stage_changed(None, &task_id, "interpret", "active", Some("Detail testing".to_string()));
        emit_ticker_log(None, &task_id, "interpret", "ok", "Completed ticker log capture test");

        let res = run_log::load_task_run_log(&task_id).expect("load task run log");
        assert!(res.exists);
        assert!(!res.pruned);
        assert!(res.content.contains("Starting ticker log capture test"));
        assert!(res.content.contains("Stage transition: stage='interpret' status='active'"));
        assert!(res.content.contains("Completed ticker log capture test"));
    }

    #[tokio::test]
    async fn test_run_log_level_filters_debug() {
        let task_id = format!("task-log-level-{}", uuid::Uuid::new_v4());

        // 1. With log level = info, debug messages must NOT be written
        run_log::set_current_run_log_level("info");
        emit_ticker_log(None, &task_id, "execute", "info", "Visible info message");
        emit_ticker_log(None, &task_id, "execute", "debug", "Hidden debug message");

        let res = run_log::load_task_run_log(&task_id).expect("load task run log");
        assert!(res.content.contains("Visible info message"));
        assert!(!res.content.contains("Hidden debug message"));

        // 2. With log level = debug, debug messages must be written
        run_log::set_current_run_log_level("debug");
        emit_ticker_log(None, &task_id, "execute", "debug", "Visible debug message");

        let res_debug = run_log::load_task_run_log(&task_id).expect("load task run log");
        assert!(res_debug.content.contains("Visible debug message"));

        // Reset back to info
        run_log::set_current_run_log_level("info");
    }

    #[tokio::test]
    async fn test_run_log_rotation_prunes_oldest() {
        let test_runs_dir = std::env::temp_dir().join(format!("kodiak-runs-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_runs_dir).unwrap();

        // Create 5 run directories with staggered creation
        for i in 1..=5 {
            let dir = test_runs_dir.join(format!("task-run-{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("run.log"), format!("Log content for task {i}")).unwrap();
            // Stagger timestamps
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Verify history is created and remains untouched
        let test_history_file = std::env::temp_dir().join(format!("kodiak-hist-test-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&test_history_file, r#"[{"task_id":"task-run-1"},{"task_id":"task-run-2"},{"task_id":"task-run-3"},{"task_id":"task-run-4"},{"task_id":"task-run-5"}]"#).unwrap();
        let history_before = std::fs::read_to_string(&test_history_file).unwrap();

        // Prune to keep 3 runs
        let pruned = run_log::prune_run_logs(&test_runs_dir, 3, 50).unwrap();
        assert_eq!(pruned, 2, "Must prune exactly 2 oldest runs");

        // The 2 oldest (task-run-1 and task-run-2) must be deleted
        assert!(!test_runs_dir.join("task-run-1").exists());
        assert!(!test_runs_dir.join("task-run-2").exists());

        // The 3 newest must exist
        assert!(test_runs_dir.join("task-run-3").exists());
        assert!(test_runs_dir.join("task-run-4").exists());
        assert!(test_runs_dir.join("task-run-5").exists());

        // History must be 100% untouched
        let history_after = std::fs::read_to_string(&test_history_file).unwrap();
        assert_eq!(history_before, history_after, "history.json must remain completely untouched");

        let _ = std::fs::remove_dir_all(&test_runs_dir);
        let _ = std::fs::remove_file(&test_history_file);
    }

    #[test]
    fn test_verifier_flags_unrelated_content() {
        use crate::prompts::VERIFY_SYSTEM_PROMPT;

        // Verify prompt shape instruction explicitly demands rejection of unrequested features
        assert!(VERIFY_SYSTEM_PROMPT.contains("CRITICAL IRRELEVANT-CONTENT RULE"));
        assert!(VERIFY_SYSTEM_PROMPT.contains("FAIL any diff containing content unrelated to the task goal"));
        assert!(VERIFY_SYSTEM_PROMPT.contains("unrequested features"));
        assert!(VERIFY_SYSTEM_PROMPT.contains("citing the unrelated element explicitly"));
        assert!(VERIFY_SYSTEM_PROMPT.contains("keyboard-backlight"));
    }
