//! Tiered AI Pipeline (Interpret → Local Execute → Cloud Verify) & Service Health (§1, §2, §4, §5).

use crate::config::{
    cloud_debug_file_path, expand_tilde, journal_file_path, resolve_projects_dir, resolve_wiki_dir,
    AppConfig, GrantLevel, WhitelistEntry,
};
use crate::enforcer::{
    check_command_deny_list, check_path_deny_list, evaluate_action, get_pipeline_generated_files,
    is_readonly_command, log_decision, resolve_canonical_target, ActionClass, EnforcementDecision,
};
use crate::events::{emit_pipeline_event, emit_ticker_log, PipelineEvent};
use crate::history::{
    upsert_task_record, ActionCard, StepExecutionLog, TaskPlan, TaskRecord,
    VerificationResult,
};
use crate::memory::{
    build_cloud_memory_catalog, build_local_memory_context, load_all_memories, match_memories,
    resolve_step_memories,
};
use crate::prompts::{
    DIGEST_LOG_PROMPT_PREFIX, DRAFT_CODE_PROMPT_PREFIX, DRAFT_DOC_PROMPT_PREFIX,
    INTERPRET_SYSTEM_PROMPT, REVIEW_GIT_DIFF_PROMPT_PREFIX, SEARCH_SUMMARIZE_PROMPT_INSTRUCTION,
    SUMMARIZE_FILE_PROMPT_PREFIX, SUMMARIZE_FILE_PROMPT_SUFFIX, VERIFY_SYSTEM_PROMPT,
};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use similar::TextDiff;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;
use uuid::Uuid;

const DEFAULT_NUM_CTX: usize = 16384;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub bridge_reachable: bool,
    pub bridge_models: Vec<String>,
    pub bridge_error: Option<String>,
    pub ollama_reachable: bool,
    pub ollama_models: Vec<String>,
    pub ollama_error: Option<String>,
    pub active_project: Option<String>,
    pub active_project_grant: Option<String>,
    pub granted_dirs_count: usize,
    pub wiki_reachable: bool,
    pub wiki_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiStatusDetails {
    pub reachable: bool,
    pub url: Option<String>,
    pub api_reachable: bool,
    pub wiki_dir: String,
    pub projects_dir: String,
    pub doc_count: usize,
    pub catalog_count: usize,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiProjectSummary {
    pub name: String,
    pub title: String,
    pub category: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub doc_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSetupInfo {
    pub wiki_enabled: bool,
    pub wiki_configured: bool,
    pub projects_dir: String,
    pub wiki_dir: String,
    pub detected_projects: Vec<String>,
    pub hosts_configured: bool,
    pub port80_configured: bool,
    pub service_active: bool,
    pub service_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSetupPayload {
    pub enable_wiki: bool,
    pub projects_dir: String,
    pub add_to_whitelist: bool,
    pub enable_service: bool,
    pub start_now: bool,
}

/// Probes Lumo bridge (`GET /v1/models`), Ollama (`GET /api/tags`), and ProjectsWiki (`127.0.0.1:80` / `:8000`).
pub async fn check_system_health(cfg: &AppConfig) -> HealthStatus {
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(2500))
        .build()
        .unwrap_or_default();

    // 1. Lumo Bridge health check (§2.1: GET http://127.0.0.1:3003/v1/models returns 200 with model list)
    let models_url = format!("{}/models", cfg.lumo_url.trim_end_matches('/'));
    let (bridge_reachable, bridge_models, bridge_error) = match client
        .get(&models_url)
        .header("Authorization", format!("Bearer {}", cfg.lumo_api_key))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<Value>().await {
                let mut models = Vec::new();
                if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
                    for item in arr {
                        if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                            models.push(id.to_string());
                        }
                    }
                }
                if models.is_empty() {
                    models = vec!["lumo".into(), "lumo-lite".into(), "lumo-max".into()];
                }
                (true, models, None)
            } else {
                (true, vec!["lumo".into()], None)
            }
        }
        Ok(resp) => (
            false,
            Vec::new(),
            Some(format!("Bridge HTTP {}", resp.status())),
        ),
        Err(e) => (false, Vec::new(), Some(format!("Bridge offline ({e})"))),
    };

    // 2. Ollama health check (`GET /api/tags`)
    let tags_url = format!("{}/api/tags", cfg.ollama_url.trim_end_matches('/'));
    let (ollama_reachable, ollama_models, ollama_error) = match client.get(&tags_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<Value>().await {
                let mut models = Vec::new();
                if let Some(arr) = body.get("models").and_then(|m| m.as_array()) {
                    for item in arr {
                        if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                            models.push(name.to_string());
                        }
                    }
                }
                (true, models, None)
            } else {
                (true, Vec::new(), None)
            }
        }
        Ok(resp) => (
            false,
            Vec::new(),
            Some(format!("Ollama HTTP {}", resp.status())),
        ),
        Err(e) => (false, Vec::new(), Some(format!("Ollama unreachable ({e})"))),
    };

    // 3. Active project & grant lookup
    let mut active_project_grant = None;
    if let Some(active) = &cfg.active_project {
        let active_exp = expand_tilde(active);
        for entry in &cfg.whitelist {
            if expand_tilde(&entry.path) == active_exp || &entry.path == active {
                active_project_grant = Some(entry.level.to_string());
                break;
            }
        }
    }

    // 4. ProjectsWiki probe (`127.0.0.1:8000` or `127.0.0.1:80` per wiki.sh)
    let mut wiki_reachable = false;
    let mut wiki_url = None;
    if cfg.wiki_enabled {
        for candidate in ["http://127.0.0.1:8000", "http://127.0.0.1:80"] {
            if let Ok(resp) = client.get(candidate).send().await {
                if resp.status().is_success() || resp.status().is_redirection() {
                    wiki_reachable = true;
                    wiki_url = Some(candidate.to_string());
                    break;
                }
            }
        }
    }

    HealthStatus {
        bridge_reachable,
        bridge_models,
        bridge_error,
        ollama_reachable,
        ollama_models,
        ollama_error,
        active_project: cfg.active_project.clone(),
        active_project_grant,
        granted_dirs_count: cfg.whitelist.len(),
        wiki_reachable,
        wiki_url,
    }
}

/// Starts `lumo-tamer` strictly via `systemctl --user start lumo-tamer` (§2.1 & §4.1),
/// then polls `GET /v1/models` until HTTP 200.
pub async fn start_lumo_bridge_service(cfg: &AppConfig) -> Result<HealthStatus, String> {
    // Check if the unit file exists or is known to systemd
    let home_unit = dirs::config_dir()
        .map(|d| d.join("systemd/user/lumo-tamer.service"))
        .unwrap_or_default();
    let unit_check = Command::new("systemctl")
        .args(["--user", "list-unit-files", "lumo-tamer.service"])
        .output();

    let unit_known = home_unit.exists()
        || unit_check
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("lumo-tamer.service"))
            .unwrap_or(false);

    if !unit_known {
        return Err(
            "systemd user unit 'lumo-tamer.service' was not found at ~/.config/systemd/user/lumo-tamer.service. \
            Per §4.1, Kodiak does not create systemd units automatically. Please create ~/.config/systemd/user/lumo-tamer.service \
            with ExecStart=%h/.gemini/config/plugins/local-delegation/scripts/tamer server and run 'systemctl --user daemon-reload'."
                .to_string(),
        );
    }

    let _ = Command::new("systemctl")
        .args(["--user", "start", "lumo-tamer"])
        .output()
        .map_err(|e| format!("Failed to invoke 'systemctl --user start lumo-tamer': {e}"))?;

    // Poll GET /v1/models up to 20 times (8 seconds total)
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let status = check_system_health(cfg).await;
        if status.bridge_reachable {
            return Ok(status);
        }
    }

    Err(
        "Invoked 'systemctl --user start lumo-tamer', but GET /v1/models did not respond within 8 seconds. \
        Check 'systemctl --user status lumo-tamer' or journalctl --user -u lumo-tamer."
            .to_string(),
    )
}

/// Starts the ProjectsWiki server via `systemctl --user start projects-wiki.service` (if present)
/// or `<wiki_dir>/wiki.sh start`, then polls until reachable.
pub async fn start_wiki_service(cfg: &AppConfig) -> Result<HealthStatus, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let projects_dir = resolve_projects_dir(cfg);
    let wiki_sh = wiki_dir.join("wiki.sh");
    let home_unit = dirs::config_dir()
        .map(|d| d.join("systemd/user/projects-wiki.service"))
        .unwrap_or_default();

    if home_unit.exists() {
        let _ = Command::new("systemctl")
            .args(["--user", "start", "projects-wiki.service"])
            .output();
    } else if wiki_sh.exists() {
        let _ = Command::new("bash")
            .arg(&wiki_sh)
            .arg("start")
            .env("PROJECTS_DIR", &projects_dir)
            .current_dir(&wiki_dir)
            .output()
            .map_err(|e| format!("Failed to run {}: {e}", wiki_sh.display()))?;
    } else {
        return Err(format!(
            "Neither ~/.config/systemd/user/projects-wiki.service nor {} was found.",
            wiki_sh.display()
        ));
    }

    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let status = check_system_health(cfg).await;
        if status.wiki_reachable {
            return Ok(status);
        }
    }

    Err("Wiki start command invoked, but neither 127.0.0.1:80 nor 127.0.0.1:8000 became reachable within 8 seconds.".to_string())
}

/// Stops the ProjectsWiki server via `systemctl --user stop projects-wiki.service` or `<wiki_dir>/wiki.sh stop`.
pub async fn stop_wiki_service(cfg: &AppConfig) -> Result<HealthStatus, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let wiki_sh = wiki_dir.join("wiki.sh");
    let home_unit = dirs::config_dir()
        .map(|d| d.join("systemd/user/projects-wiki.service"))
        .unwrap_or_default();

    if home_unit.exists() {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", "projects-wiki.service"])
            .output();
    }
    if wiki_sh.exists() {
        let _ = Command::new("bash")
            .arg(&wiki_sh)
            .arg("stop")
            .current_dir(&wiki_dir)
            .output();
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(check_system_health(cfg).await)
}

/// Restarts the ProjectsWiki server.
pub async fn restart_wiki_service(cfg: &AppConfig) -> Result<HealthStatus, String> {
    let _ = stop_wiki_service(cfg).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    start_wiki_service(cfg).await
}

/// Synchronizes the MkDocs documentation catalog by running `make_wiki.py`.
pub async fn sync_wiki_catalog_service(cfg: &AppConfig) -> Result<String, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let projects_dir = resolve_projects_dir(cfg);
    let make_wiki = wiki_dir.join("make_wiki.py");
    if !make_wiki.exists() {
        return Err(format!("make_wiki.py not found in {}", wiki_dir.display()));
    }

    let out = Command::new("python3")
        .arg(&make_wiki)
        .env("PROJECTS_DIR", &projects_dir)
        .current_dir(&wiki_dir)
        .output()
        .map_err(|e| format!("Failed to run make_wiki.py: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(format!("make_wiki.py failed: {stderr}"));
    }
    Ok(if stdout.is_empty() { "Catalog synced successfully.".to_string() } else { stdout })
}

/// Rebuilds accurate documentation and assets (manuals, photos, PDFs) via `rebuild_accurate_docs.py`.
pub async fn rebuild_wiki_assets_service(cfg: &AppConfig) -> Result<String, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let projects_dir = resolve_projects_dir(cfg);
    let rebuild_py = wiki_dir.join("rebuild_accurate_docs.py");
    if !rebuild_py.exists() {
        return Err(format!("rebuild_accurate_docs.py not found in {}", wiki_dir.display()));
    }

    let out = Command::new("python3")
        .arg(&rebuild_py)
        .env("PROJECTS_DIR", &projects_dir)
        .current_dir(&wiki_dir)
        .output()
        .map_err(|e| format!("Failed to run rebuild_accurate_docs.py: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(format!("rebuild_accurate_docs.py failed: {stderr}"));
    }
    // Also update catalog
    let _ = sync_wiki_catalog_service(cfg).await;
    Ok(if stdout.is_empty() { "Assets rebuilt and catalog synced successfully.".to_string() } else { stdout })
}

/// Generates or refreshes a project's technical documentation using local Ollama models via `build_wiki.py`.
pub async fn ai_build_wiki_project_service(
    cfg: &AppConfig,
    project_name: &str,
    model: Option<&str>,
    overwrite: bool,
) -> Result<String, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let projects_dir = resolve_projects_dir(cfg);
    let build_py = wiki_dir.join("build_wiki.py");
    if !build_py.exists() {
        return Err(format!("build_wiki.py not found in {}", wiki_dir.display()));
    }

    let chosen_model = model.unwrap_or("qwen2.5-coder:7b");
    let mut cmd = Command::new("python3");
    cmd.arg(&build_py)
        .arg("--project")
        .arg(project_name)
        .arg("--model")
        .arg(chosen_model)
        .env("PROJECTS_DIR", &projects_dir)
        .current_dir(&wiki_dir);

    if overwrite {
        cmd.arg("--overwrite");
    }

    let out = cmd.output().map_err(|e| format!("Failed to run build_wiki.py: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();

    if !out.status.success() {
        return Err(format!("AI documentation build failed: {stderr}"));
    }

    Ok(stdout)
}

/// Returns detailed status of the ProjectsWiki including doc counts, catalog items, and API reachability.
pub async fn get_detailed_wiki_status(cfg: &AppConfig) -> Result<WikiStatusDetails, String> {
    let health = check_system_health(cfg).await;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(1500))
        .build()
        .unwrap_or_default();

    let api_reachable = client
        .get("http://127.0.0.1:8765/health")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    let wiki_dir = resolve_wiki_dir(cfg);
    let docs_dir = wiki_dir.join("docs");
    let mut doc_count = 0;
    if docs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&docs_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && name != "index.md" {
                    doc_count += 1;
                }
            }
        }
    }

    let projects_json_path = docs_dir.join("projects.json");
    let mut catalog_count = 0;
    let mut categories = Vec::new();
    if projects_json_path.exists() {
        if let Ok(data) = std::fs::read_to_string(&projects_json_path) {
            if let Ok(val) = serde_json::from_str::<Value>(&data) {
                if let Some(arr) = val.as_array() {
                    catalog_count = arr.len();
                    for item in arr {
                        if let Some(cat) = item.get("category").and_then(|c| c.as_str()) {
                            if !categories.contains(&cat.to_string()) {
                                categories.push(cat.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    let projects_dir = resolve_projects_dir(cfg).to_string_lossy().to_string();

    Ok(WikiStatusDetails {
        reachable: health.wiki_reachable,
        url: health.wiki_url,
        api_reachable,
        wiki_dir: wiki_dir.to_string_lossy().to_string(),
        projects_dir,
        doc_count,
        catalog_count,
        categories,
    })
}

/// Returns the catalog of all project entries from `projects.json`.
pub async fn list_wiki_projects_service(cfg: &AppConfig) -> Result<Vec<Value>, String> {
    let wiki_dir = resolve_wiki_dir(cfg);
    let projects_json = wiki_dir.join("docs/projects.json");
    if projects_json.exists() {
        let content = std::fs::read_to_string(&projects_json)
            .map_err(|e| format!("Failed to read projects.json: {e}"))?;
        let items: Vec<Value> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse projects.json: {e}"))?;
        return Ok(items);
    }
    Ok(Vec::new())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RescanResult {
    pub projects_dir: String,
    pub total_projects: usize,
    pub added_count: usize,
    pub added_projects: Vec<String>,
}

fn is_ignored_project_dir_name(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "venv"
                | ".venv"
                | "__pycache__"
                | "dist"
                | "build"
                | "pkg"
                | ".git"
        )
}

fn dir_contains_project_markers(path: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".ino")
                || file_name == "Cargo.toml"
                || file_name == "package.json"
                || file_name == "Makefile"
                || file_name == "CMakeLists.txt"
                || file_name == "platformio.ini"
                || file_name == "pyproject.toml"
                || file_name == "requirements.txt"
                || file_name == ".git"
            {
                return true;
            }
        }
    }
    false
}

/// Automatically or manually rescans the configured projects directory, discovering
/// new project folders (including Arduino .ino sketches, git repos, etc.) and synchronizing
/// them into Kodiak's security whitelist with READ grant level.
pub async fn rescan_projects_service(cfg: &mut AppConfig) -> Result<RescanResult, String> {
    let p_dir = resolve_projects_dir(cfg);
    if !p_dir.exists() || !p_dir.is_dir() {
        return Err(format!(
            "Projects directory '{}' does not exist or is not a directory.",
            p_dir.display()
        ));
    }

    let mut candidate_paths: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&p_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_ignored_project_dir_name(&name) {
                    continue;
                }
                candidate_paths.push(path.clone());

                // Also check 1 level down if this directory contains sub-projects (e.g. Arduino sketches or categorized projects)
                if !dir_contains_project_markers(&path) {
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            let sub_path = sub_entry.path();
                            if sub_path.is_dir() {
                                let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                                if !is_ignored_project_dir_name(&sub_name)
                                    && dir_contains_project_markers(&sub_path)
                                {
                                    candidate_paths.push(sub_path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut added_projects = Vec::new();
    let exclusion_set: std::collections::HashSet<String> = cfg.exclusion_list.iter().cloned().collect();

    for cand in candidate_paths {
        let cand_str = cand.to_string_lossy().to_string();
        let cand_name = cand
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cand_str.clone());

        // Skip if explicitly excluded
        if exclusion_set.contains(&cand_str) || exclusion_set.contains(&cand_name) {
            continue;
        }

        let norm_cand = cand_str.trim_end_matches('/');

        // Check if already in whitelist
        let already_present = cfg.whitelist.iter().any(|w| {
            let norm_w = w.path.trim_end_matches('/');
            norm_w == norm_cand
        });

        if !already_present {
            cfg.whitelist.push(WhitelistEntry {
                path: cand_str.clone(),
                level: GrantLevel::Read,
            });
            added_projects.push(cand_name);
        }
    }

    if cfg.active_project.is_none() && !cfg.whitelist.is_empty() {
        cfg.active_project = Some(cfg.whitelist[0].path.clone());
    }

    let added_count = added_projects.len();

    if cfg.wiki_enabled {
        let _ = sync_wiki_catalog_service(cfg).await;
    }

    Ok(RescanResult {
        projects_dir: p_dir.to_string_lossy().to_string(),
        total_projects: cfg.whitelist.len(),
        added_count,
        added_projects,
    })
}

/// Scans a directory and returns names of project folders contained within it.
pub fn detect_projects_in_dir_service(dir_path: &str) -> Vec<String> {
    let path = expand_tilde(dir_path);
    let mut detected = Vec::new();
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !is_ignored_project_dir_name(&name) {
                        detected.push(name);
                    }
                }
            }
        }
    }
    detected.sort();
    detected
}

/// Probes system host configuration, unprivileged port status, and detected projects for setup wizard.
pub async fn get_wiki_setup_info_service(cfg: &AppConfig) -> Result<WikiSetupInfo, String> {
    let p_dir = resolve_projects_dir(cfg);
    let detected_projects = detect_projects_in_dir_service(&p_dir.to_string_lossy());

    let hosts_configured = if let Ok(content) = std::fs::read_to_string("/etc/hosts") {
        content.lines().any(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.contains("wiki.local")
        })
    } else {
        false
    };

    let port80_configured = if let Ok(val) = std::fs::read_to_string("/proc/sys/net/ipv4/ip_unprivileged_port_start") {
        val.trim().parse::<u32>().map(|p| p <= 80).unwrap_or(false)
    } else {
        false
    };

    let home_unit = dirs::config_dir()
        .map(|d| d.join("systemd/user/projects-wiki.service"))
        .unwrap_or_default();
    let service_enabled = home_unit.exists() && {
        Command::new("systemctl")
            .args(["--user", "is-enabled", "projects-wiki.service"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
            .unwrap_or(false)
    };
    let service_active = Command::new("systemctl")
        .args(["--user", "is-active", "projects-wiki.service"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);

    let wiki_dir = resolve_wiki_dir(cfg);

    Ok(WikiSetupInfo {
        wiki_enabled: cfg.wiki_enabled,
        wiki_configured: cfg.wiki_configured,
        projects_dir: p_dir.to_string_lossy().to_string(),
        wiki_dir: wiki_dir.to_string_lossy().to_string(),
        detected_projects,
        hosts_configured,
        port80_configured,
        service_active,
        service_enabled,
    })
}

/// Completes or skips the ProjectsWiki setup wizard, saving paths and updating whitelist if selected.
pub async fn complete_wiki_setup_service(
    cfg: &mut AppConfig,
    payload: WikiSetupPayload,
) -> Result<AppConfig, String> {
    cfg.wiki_configured = true;
    cfg.wiki_enabled = payload.enable_wiki;

    if payload.enable_wiki {
        if !payload.projects_dir.trim().is_empty() {
            cfg.projects_dir = payload.projects_dir.trim().to_string();
        }

        let p_dir = resolve_projects_dir(cfg);

        // Optionally add discovered projects to Kodiak's whitelist with READ grant
        if payload.add_to_whitelist && p_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p_dir) {
                for entry in entries.flatten() {
                    if let Ok(ft) = entry.file_type() {
                        if ft.is_dir() {
                            let path_str = entry.path().to_string_lossy().to_string();
                            let name = entry.file_name().to_string_lossy().to_string();
                            if !name.starts_with('.') && name != "node_modules" && name != "target" && name != "venv" && name != "__pycache__" {
                                if !cfg.whitelist.iter().any(|w| w.path == path_str) {
                                    cfg.whitelist.push(WhitelistEntry {
                                        path: path_str.clone(),
                                        level: GrantLevel::Read,
                                    });
                                }
                                if cfg.active_project.is_none() {
                                    cfg.active_project = Some(path_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Configure systemd service if requested
        if payload.enable_service {
            let wiki_dir = resolve_wiki_dir(cfg);
            let wiki_sh = wiki_dir.join("wiki.sh");
            if let Some(config_dir) = dirs::config_dir() {
                let service_path = config_dir.join("systemd/user/projects-wiki.service");
                let unit = format!(
                    "[Unit]\nDescription=ProjectsWiki Local Server (http://wiki.local)\nAfter=network.target\n\n\
                    [Service]\nType=simple\nWorkingDirectory={}\nExecStart={} daemon\nRestart=on-failure\nRestartSec=3\n\n\
                    [Install]\nWantedBy=default.target\n",
                    wiki_dir.display(),
                    wiki_sh.display()
                );
                if let Some(parent) = service_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&service_path, unit);
                let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).output();
                let _ = Command::new("systemctl").args(["--user", "enable", "projects-wiki.service"]).output();
            }
        }

        // Initial sync of the catalog
        let _ = sync_wiki_catalog_service(cfg).await;

        // Start server now if requested
        if payload.start_now {
            let _ = start_wiki_service(cfg).await;
        }
    } else {
        let _ = stop_wiki_service(cfg).await;
    }

    Ok(cfg.clone())
}

/// Resolves requested Ollama model against installed models (`ollama_delegate_mcp.py` L152-L167).
async fn resolve_ollama_model(cfg: &AppConfig, requested: &str) -> String {
    let health = check_system_health(cfg).await;
    if !health.ollama_models.is_empty() {
        if health.ollama_models.iter().any(|m| m == requested) {
            return requested.to_string();
        }
        for fallback in [&cfg.coder_model, &cfg.heavy_model, &cfg.fast_model] {
            if health.ollama_models.iter().any(|m| m == fallback) {
                return fallback.clone();
            }
        }
    }
    requested.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudRequestDebugEntry {
    pub timestamp: String,
    pub stage: String,
    pub attempt: u8,
    pub url: String,
    pub model: String,
    pub message_count: usize,
    pub system_prompt_bytes: usize,
    pub user_prompt_bytes: usize,
    pub total_payload_bytes: usize,
    pub stream: bool,
    pub has_temperature: bool,
    pub has_max_tokens: bool,
    pub status_code: Option<u16>,
    pub elapsed_ms: u128,
    pub outcome: String,
    pub detail: String,
}

fn append_cloud_debug_log(entry: &CloudRequestDebugEntry) {
    if let Ok(path) = cloud_debug_file_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(line) = serde_json::to_string(entry) {
                use std::io::Write;
                let _ = writeln!(file, "{line}");
            }
        }
    }
    eprintln!(
        "[Kodiak Cloud Debug] stage={} attempt={} payload={}B (sys={}B, user={}B) status={:?} elapsed={}ms outcome={} detail={}",
        entry.stage,
        entry.attempt,
        entry.total_payload_bytes,
        entry.system_prompt_bytes,
        entry.user_prompt_bytes,
        entry.status_code,
        entry.elapsed_ms,
        entry.outcome,
        entry.detail
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudErrorKind {
    QuotaExhausted,
    ServerError,
    ConnectionRefused,
    BadRequest,
    Unknown,
}

pub fn classify_cloud_error(status: Option<u16>, body_or_msg: &str) -> CloudErrorKind {
    if status == Some(429) {
        return CloudErrorKind::QuotaExhausted;
    }
    let lower = body_or_msg.to_ascii_lowercase();
    if lower.contains("\"type\":\"quota_exhausted\"")
        || lower.contains("\"code\":\"quota_exhausted\"")
        || lower.contains("quota_exhausted")
    {
        return CloudErrorKind::QuotaExhausted;
    }
    let has_quota_keywords = lower.contains("quota")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("usage limit")
        || lower.contains("usage_limit");

    if has_quota_keywords {
        if status == Some(429) || status == Some(500) || status == Some(403) || status.is_none()
            || lower.contains("429") || lower.contains("500") || lower.contains("http 500") || lower.contains("http 429")
        {
            return CloudErrorKind::QuotaExhausted;
        }
    }

    if let Some(code) = status {
        if (500..=599).contains(&code) {
            return CloudErrorKind::ServerError;
        }
        if (400..=499).contains(&code) {
            return CloudErrorKind::BadRequest;
        }
    } else if lower.contains("http 5") || lower.contains("500 internal server error") || lower.contains("server_error") {
        return CloudErrorKind::ServerError;
    }

    if lower.contains("connection refused") || lower.contains("connection_refused") || lower.contains("connect: connection refused") {
        return CloudErrorKind::ConnectionRefused;
    }

    CloudErrorKind::Unknown
}

pub fn classify_cloud_error_from_str(text: &str) -> CloudErrorKind {
    let status = if text.contains("429") {
        Some(429)
    } else if text.contains("500") {
        Some(500)
    } else if text.contains("502") {
        Some(502)
    } else if text.contains("503") {
        Some(503)
    } else {
        None
    };
    classify_cloud_error(status, text)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageProvider {
    DefaultCloud,
    LocalHeavy,
    Alternate(crate::config::AltProviderConfig),
}

fn format_reqwest_error(e: &reqwest::Error) -> String {
    let mut err_str = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        let msg = s.to_string();
        if !err_str.contains(&msg) {
            err_str.push_str(&format!(" ({msg})"));
        }
        source = s.source();
    }
    err_str
}

pub async fn call_alt_provider_chat(
    provider: &crate::config::AltProviderConfig,
    _stage: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let timeout_secs = provider.timeout_secs.unwrap_or(180).max(30);
    let mut builder = Client::builder().timeout(Duration::from_secs(timeout_secs));
    if provider.base_url.contains("127.0.0.1") || provider.base_url.contains("localhost") {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| format!("HTTP client error: {e}"))?;

    let trimmed_base = provider.base_url.trim_end_matches('/');
    let url = if trimmed_base.ends_with("/chat/completions") {
        trimmed_base.to_string()
    } else {
        format!("{trimmed_base}/chat/completions")
    };

    let bounded_user_prompt = if let Some(max_chars) = provider.max_context_chars {
        if user_prompt.len() > max_chars {
            &user_prompt[..max_chars]
        } else {
            user_prompt
        }
    } else {
        user_prompt
    };

    let payload = json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": bounded_user_prompt }
        ],
        "stream": false
    });

    let mut req = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json");

    if provider.base_url.contains("generativelanguage.googleapis.com") {
        req = req.header("x-goog-api-key", &provider.api_key);
    }

    let resp = req
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Alternate provider '{}' unreachable at {url}: {}", provider.name, format_reqwest_error(&e)))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Alternate provider '{}' returned HTTP {status}: {text}", provider.name));
    }

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid JSON from alternate provider '{}': {e}", provider.name))?;

    let content = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|cnt| cnt.as_str())
        .ok_or_else(|| format!("Alternate provider '{}' response missing choices[0].message.content: {body}", provider.name))?;

    Ok(content.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTestResult {
    pub success: bool,
    pub status_code: u16,
    pub message: String,
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

pub async fn ping_alt_provider(provider: &crate::config::AltProviderConfig) -> Result<ProviderTestResult, String> {
    let timeout_secs = provider.timeout_secs.unwrap_or(30).min(45).max(10);
    let mut builder = Client::builder().timeout(Duration::from_secs(timeout_secs));
    if provider.base_url.contains("127.0.0.1") || provider.base_url.contains("localhost") {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| format!("HTTP client error: {e}"))?;

    let trimmed_base = provider.base_url.trim_end_matches('/');
    let url = if trimmed_base.ends_with("/chat/completions") {
        trimmed_base.to_string()
    } else {
        format!("{trimmed_base}/chat/completions")
    };

    let payload = json!({
        "model": provider.model,
        "messages": [
            { "role": "user", "content": "ping" }
        ],
        "max_tokens": 1,
        "stream": false
    });

    let start_instant = std::time::Instant::now();
    let mut req = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json");

    if provider.base_url.contains("generativelanguage.googleapis.com") {
        req = req.header("x-goog-api-key", &provider.api_key);
    }

    match req
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            let latency_ms = start_instant.elapsed().as_millis() as u64;
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                Ok(ProviderTestResult {
                    success: true,
                    status_code: status,
                    message: format!("HTTP {status} OK ({latency_ms}ms): Connection successful"),
                    latency_ms: Some(latency_ms),
                })
            } else {
                let err_text = resp.text().await.unwrap_or_default();
                let short_err = if err_text.len() > 120 {
                    format!("{}...", &err_text[..117])
                } else {
                    err_text
                };
                Ok(ProviderTestResult {
                    success: false,
                    status_code: status,
                    message: format!("HTTP {status} ({latency_ms}ms): {short_err}"),
                    latency_ms: Some(latency_ms),
                })
            }
        }
        Err(e) => {
            let latency_ms = start_instant.elapsed().as_millis() as u64;
            let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
            Ok(ProviderTestResult {
                success: false,
                status_code: status,
                message: format!("Connection failed ({latency_ms}ms): {}", format_reqwest_error(&e)),
                latency_ms: Some(latency_ms),
            })
        }
    }
}

pub async fn fetch_provider_models(
    base_url: &str,
    api_key: &str,
    provider_name: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(12));
    if base_url.contains("127.0.0.1") || base_url.contains("localhost") {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| format!("HTTP client error: {e}"))?;

    let base_trimmed = base_url.trim().trim_end_matches('/');
    let prov_name_lower = provider_name.unwrap_or("").to_lowercase();
    let is_gemini = base_trimmed.contains("generativelanguage.googleapis.com")
        || prov_name_lower.contains("gemini");

    // 1. If Gemini / Google Generative Language
    if is_gemini && !api_key.trim().is_empty() {
        let native_url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            api_key.trim()
        );
        if let Ok(resp) = client.get(&native_url).send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(models_arr) = body.get("models").and_then(|v| v.as_array()) {
                        let mut results: Vec<String> = models_arr
                            .iter()
                            .filter_map(|m| {
                                let name = m.get("name")?.as_str()?;
                                let clean_name = name.strip_prefix("models/").unwrap_or(name);
                                let methods = m.get("supportedGenerationMethods")?.as_array()?;
                                let can_generate = methods.iter().any(|meth| meth.as_str() == Some("generateContent"));
                                if can_generate
                                    && !clean_name.contains("embedding")
                                    && !clean_name.contains("aqa")
                                    && !clean_name.contains("imagen")
                                    && !clean_name.contains("2.5-flash")
                                {
                                    Some(clean_name.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !results.is_empty() {
                            results.sort_by(|a, b| {
                                let score = |s: &str| -> i32 {
                                    if s.contains("flash-lite") { 0 }
                                    else if s.contains("flash-latest") { 1 }
                                    else if s.contains("3.8") { 2 }
                                    else if s.contains("3.0") { 3 }
                                    else if s.contains("2.5-pro") { 4 }
                                    else if s.contains("1.5-pro") { 5 }
                                    else if s.contains("2.0") { 6 }
                                    else if s.contains("1.5-flash") { 7 }
                                    else { 8 }
                                };
                                score(a).cmp(&score(b)).then(a.cmp(b))
                            });
                            return Ok(results);
                        }
                    }
                }
            }
        }
    }

    // 2. Standard OpenAI-compatible /models endpoint
    let models_url = if base_trimmed.ends_with("/models") {
        base_trimmed.to_string()
    } else {
        format!("{base_trimmed}/models")
    };

    let mut req = client.get(&models_url);
    if !api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    if let Ok(resp) = req.send().await {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data_arr) = body.get("data").and_then(|v| v.as_array()) {
                    let list: Vec<String> = data_arr
                        .iter()
                        .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
                        .filter(|id| {
                            let lower = id.to_lowercase();
                            !lower.contains("whisper")
                                && !lower.contains("embedding")
                                && !lower.contains("tts")
                                && !lower.contains("dall-e")
                                && !lower.contains("moderation")
                        })
                        .collect();
                    if !list.is_empty() {
                        return Ok(list);
                    }
                }
            }
        }
    }

    // 3. Fallback for Ollama: try /api/tags
    let host_base = if let Some(idx) = base_trimmed.find("/v1") {
        &base_trimmed[..idx]
    } else {
        base_trimmed
    };
    let ollama_tags_url = format!("{host_base}/api/tags");
    if let Ok(resp) = client.get(&ollama_tags_url).send().await {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(models_arr) = body.get("models").and_then(|v| v.as_array()) {
                    let list: Vec<String> = models_arr
                        .iter()
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                        .collect();
                    if !list.is_empty() {
                        return Ok(list);
                    }
                }
            }
        }
    }

    Err(format!(
        "Failed to fetch model list from '{base_trimmed}'. Verify endpoint URL and API key."
    ))
}

pub async fn call_local_stage_chat(
    cfg: &AppConfig,
    _stage: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let full_prompt = format!("{system_prompt}\n\n{user_prompt}");
    let (_model_used, reply) = call_ollama_generate(cfg, &cfg.heavy_model, &full_prompt, 0.1).await?;
    Ok(reply)
}

pub async fn probe_cloud_health(cfg: &AppConfig) -> Result<bool, String> {
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // 1. Cheap model-list ping
    let models_url = format!("{}/models", cfg.lumo_url.trim_end_matches('/'));
    let ping_res = client
        .get(&models_url)
        .header("Authorization", format!("Bearer {}", cfg.lumo_api_key))
        .send()
        .await
        .map_err(|e| format!("Model ping error: {e}"))?;

    if !ping_res.status().is_success() {
        let status = ping_res.status();
        let text = ping_res.text().await.unwrap_or_default();
        return Err(format!("Model list ping failed with HTTP {status}: {text}"));
    }

    // 2. One-token completion probe
    let chat_url = format!("{}/chat/completions", cfg.lumo_url.trim_end_matches('/'));
    let probe_payload = json!({
        "model": cfg.cloud_model,
        "messages": [
            { "role": "user", "content": "ping" }
        ],
        "max_tokens": 1,
        "stream": false
    });

    let chat_res = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", cfg.lumo_api_key))
        .header("Content-Type", "application/json")
        .json(&probe_payload)
        .send()
        .await
        .map_err(|e| format!("Probe completion error: {e}"))?;

    let status = chat_res.status();
    let text = chat_res.text().await.unwrap_or_default();
    let kind = classify_cloud_error(Some(status.as_u16()), &text);
    if kind == CloudErrorKind::QuotaExhausted || !status.is_success() {
        return Err(format!("Probe completion returned HTTP {status}: {text}"));
    }

    Ok(true)
}

/// Calls Lumo Bridge (`POST /v1/chat/completions`) (`ollama_delegate_mcp.py` L81-L116).
/// Includes debug-level request/response shape logging and automatic single retry on 5xx / transient socket reset after a 2 s backoff (§1 Slice 2).
pub async fn call_lumo_chat(
    cfg: &AppConfig,
    stage: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(cfg.command_timeout_secs.max(30)))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let url = format!("{}/chat/completions", cfg.lumo_url.trim_end_matches('/'));
    let model = match cfg.cloud_model.trim().to_ascii_lowercase().as_str() {
        "lumo" | "lumo-lite" | "lumo-max" => cfg.cloud_model.trim().to_ascii_lowercase(),
        _ => "lumo".to_string(),
    };

    let payload = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "stream": false
    });

    let serialized_payload = serde_json::to_string(&payload).unwrap_or_default();
    let total_payload_bytes = serialized_payload.len();
    let system_prompt_bytes = system_prompt.len();
    let user_prompt_bytes = user_prompt.len();

    let max_attempts: u8 = 2; // Initial attempt + 1 automatic retry on 5xx / transient error after 2s backoff
    let mut last_err = String::new();

    for attempt in 1..=max_attempts {
        let started = std::time::Instant::now();
        let send_res = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", cfg.lumo_api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await;

        let elapsed_ms = started.elapsed().as_millis();

        match send_res {
            Err(e) => {
                let err_msg = format!("Cloud Lumo Bridge unreachable at {url}: {e}");
                append_cloud_debug_log(&CloudRequestDebugEntry {
                    timestamp: Utc::now().to_rfc3339(),
                    stage: stage.to_string(),
                    attempt,
                    url: url.clone(),
                    model: model.clone(),
                    message_count: 2,
                    system_prompt_bytes,
                    user_prompt_bytes,
                    total_payload_bytes,
                    stream: false,
                    has_temperature: false,
                    has_max_tokens: false,
                    status_code: None,
                    elapsed_ms,
                    outcome: if attempt < max_attempts {
                        "transport_error_retrying".to_string()
                    } else {
                        "transport_error_failed".to_string()
                    },
                    detail: err_msg.clone(),
                });
                last_err = err_msg;
                if attempt < max_attempts {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                return Err(last_err);
            }
            Ok(resp) => {
                let status = resp.status();
                let status_code = status.as_u16();
                if status.is_server_error() {
                    let text = resp.text().await.unwrap_or_default();
                    let kind = classify_cloud_error(Some(status_code), &text);
                    if kind == CloudErrorKind::QuotaExhausted {
                        let err_msg = format!("Cloud Lumo Bridge returned HTTP {status} (quota_exhausted): {text}");
                        append_cloud_debug_log(&CloudRequestDebugEntry {
                            timestamp: Utc::now().to_rfc3339(),
                            stage: stage.to_string(),
                            attempt,
                            url: url.clone(),
                            model: model.clone(),
                            message_count: 2,
                            system_prompt_bytes,
                            user_prompt_bytes,
                            total_payload_bytes,
                            stream: false,
                            has_temperature: false,
                            has_max_tokens: false,
                            status_code: Some(status_code),
                            elapsed_ms,
                            outcome: "http_quota_exhausted_no_retry".to_string(),
                            detail: text,
                        });
                        return Err(err_msg);
                    }
                    let err_msg = format!("Cloud Lumo Bridge returned HTTP {status}: {text}");
                    append_cloud_debug_log(&CloudRequestDebugEntry {
                        timestamp: Utc::now().to_rfc3339(),
                        stage: stage.to_string(),
                        attempt,
                        url: url.clone(),
                        model: model.clone(),
                        message_count: 2,
                        system_prompt_bytes,
                        user_prompt_bytes,
                        total_payload_bytes,
                        stream: false,
                        has_temperature: false,
                        has_max_tokens: false,
                        status_code: Some(status_code),
                        elapsed_ms,
                        outcome: if attempt < max_attempts {
                            "http_5xx_retrying_after_2s".to_string()
                        } else {
                            "http_5xx_failed".to_string()
                        },
                        detail: text,
                    });
                    last_err = err_msg;
                    if attempt < max_attempts {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    return Err(last_err);
                }

                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let kind = classify_cloud_error(Some(status_code), &text);
                    let (outcome, err_msg) = if kind == CloudErrorKind::QuotaExhausted {
                        ("http_429_quota_exhausted".to_string(), format!("Cloud Lumo Bridge returned HTTP {status} (quota_exhausted): {text}"))
                    } else {
                        ("http_4xx_failed".to_string(), format!("Cloud Lumo Bridge returned HTTP {status}: {text}"))
                    };
                    append_cloud_debug_log(&CloudRequestDebugEntry {
                        timestamp: Utc::now().to_rfc3339(),
                        stage: stage.to_string(),
                        attempt,
                        url: url.clone(),
                        model: model.clone(),
                        message_count: 2,
                        system_prompt_bytes,
                        user_prompt_bytes,
                        total_payload_bytes,
                        stream: false,
                        has_temperature: false,
                        has_max_tokens: false,
                        status_code: Some(status_code),
                        elapsed_ms,
                        outcome,
                        detail: text,
                    });
                    return Err(err_msg);
                }

                let body: Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("Invalid JSON from Lumo Bridge: {e}"))?;

                if let Some(content) = body
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("message"))
                    .and_then(|msg| msg.get("content"))
                    .and_then(|c| c.as_str())
                {
                    let mut preview: String = content.chars().take(160).collect();
                    if content.chars().count() > 160 {
                        preview.push_str("...");
                    }
                    append_cloud_debug_log(&CloudRequestDebugEntry {
                        timestamp: Utc::now().to_rfc3339(),
                        stage: stage.to_string(),
                        attempt,
                        url: url.clone(),
                        model: model.clone(),
                        message_count: 2,
                        system_prompt_bytes,
                        user_prompt_bytes,
                        total_payload_bytes,
                        stream: false,
                        has_temperature: false,
                        has_max_tokens: false,
                        status_code: Some(status_code),
                        elapsed_ms,
                        outcome: "ok".to_string(),
                        detail: format!("reply_bytes={} preview={:?}", content.len(), preview),
                    });
                    return Ok(content.to_string());
                } else {
                    let err_msg = format!("Unexpected Lumo Bridge response structure: {body}");
                    append_cloud_debug_log(&CloudRequestDebugEntry {
                        timestamp: Utc::now().to_rfc3339(),
                        stage: stage.to_string(),
                        attempt,
                        url: url.clone(),
                        model: model.clone(),
                        message_count: 2,
                        system_prompt_bytes,
                        user_prompt_bytes,
                        total_payload_bytes,
                        stream: false,
                        has_temperature: false,
                        has_max_tokens: false,
                        status_code: Some(status_code),
                        elapsed_ms,
                        outcome: "malformed_response".to_string(),
                        detail: err_msg.clone(),
                    });
                    return Err(err_msg);
                }
            }
        }
    }

    Err(last_err)
}

pub fn classify_ollama_error(url: &str, err: &reqwest::Error, total_timeout_secs: u64) -> String {
    if err.is_connect() {
        format!("Local Ollama connection refused at {url} (connection_refused) — service is down or not listening. Verify Ollama is running.")
    } else if err.is_timeout() {
        format!(
            "Local generation canceled: exceeded total timeout of {}s — server was still generating (client disconnect). Raise local_total_timeout_secs or simplify the step. (client_timeout_total)",
            total_timeout_secs
        )
    } else {
        format!("Local Ollama request failed at {url}: {err}")
    }
}

/// Calls local Ollama (`POST /api/generate`) with `num_ctx: 16384` and `temperature: 0.1` (`ollama_delegate_mcp.py` L118-L149).
pub async fn call_ollama_generate(
    cfg: &AppConfig,
    requested_model: &str,
    prompt: &str,
    temperature: f64,
) -> Result<(String, String), String> {
    let model = resolve_ollama_model(cfg, requested_model).await;
    let client = Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(cfg.local_total_timeout_secs))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let url = format!("{}/api/generate", cfg.ollama_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "num_ctx": DEFAULT_NUM_CTX,
            "temperature": temperature
        }
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| classify_ollama_error(&url, &e, cfg.local_total_timeout_secs))?;

    if resp.status().is_server_error() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Local Ollama returned HTTP {status} (server_error): {text}"));
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Local Ollama returned HTTP {status}: {text}"));
    }

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid JSON from Ollama: {e}"))?;

    let reply = body
        .get("response")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    Ok((model, reply))
}

/// Calls local Ollama (`POST /api/generate`) with `"stream": true` for `draft_code` (§2.3 Slice 2).
/// Streams token chunks into `PipelineEvent::DraftStreamChunk` in real time.
/// If streaming fails before any bytes arrive (e.g., non-streaming proxy), falls back gracefully to `call_ollama_generate`.
/// If Ollama disconnects or is killed mid-stream, emits `DraftStreamDone { success: false }` and returns `Err(...)`
/// so zero orphaned partial writes ever touch disk.
pub async fn call_ollama_generate_streaming(
    app: Option<&tauri::AppHandle>,
    cfg: &AppConfig,
    task_id: &str,
    step_id: usize,
    title: &str,
    action_class_str: &str,
    target_path: &str,
    requested_model: &str,
    prompt: &str,
    temperature: f64,
) -> Result<(String, String), String> {
    let model = resolve_ollama_model(cfg, requested_model).await;
    let client = Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let url = format!("{}/api/generate", cfg.ollama_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
        "options": {
            "num_ctx": DEFAULT_NUM_CTX,
            "temperature": temperature
        }
    });

    let mut resp = match client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let err_msg = classify_ollama_error(&url, &e, cfg.local_total_timeout_secs);
            emit_pipeline_event(
                app,
                PipelineEvent::DraftStreamDone {
                    task_id: task_id.to_string(),
                    step_id,
                    target_path: target_path.to_string(),
                    success: false,
                    error: Some(err_msg.clone()),
                },
            );
            return Err(err_msg);
        }
    };

    if resp.status().is_server_error() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let err_msg = format!("Local Ollama returned HTTP {status} (server_error): {text}");
        emit_pipeline_event(
            app,
            PipelineEvent::DraftStreamDone {
                task_id: task_id.to_string(),
                step_id,
                target_path: target_path.to_string(),
                success: false,
                error: Some(err_msg.clone()),
            },
        );
        return Err(err_msg);
    }

    if !resp.status().is_success() {
        // Graceful fallback to non-streaming complete-on-finish if endpoint rejected stream:true
        return call_ollama_generate(cfg, requested_model, prompt, temperature).await;
    }

    let mut accumulated = String::new();
    let mut line_buf = String::new();
    let mut saw_done = false;

    let idle_limit = Duration::from_secs(cfg.local_idle_timeout_secs);

    loop {
        match timeout(idle_limit, resp.chunk()).await {
            Err(_) => {
                let err_msg = format!(
                    "Local generation idle timeout: no chunks received for {}s (client_timeout_idle) — server hung or stopped responding. Raise local_idle_timeout_secs if model loading or processing takes longer.",
                    cfg.local_idle_timeout_secs
                );
                emit_pipeline_event(
                    app,
                    PipelineEvent::DraftStreamDone {
                        task_id: task_id.to_string(),
                        step_id,
                        target_path: target_path.to_string(),
                        success: false,
                        error: Some(err_msg.clone()),
                    },
                );
                return Err(err_msg);
            }
            Ok(Ok(Some(bytes))) => {
                let piece = String::from_utf8_lossy(&bytes);
                line_buf.push_str(&piece);

                while let Some(nl_idx) = line_buf.find('\n') {
                    let line: String = line_buf.drain(..=nl_idx).collect();
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                        if let Some(err_str) = val.get("error").and_then(|v| v.as_str()) {
                            let err_msg = format!("Local Ollama error mid-stream: {err_str}");
                            emit_pipeline_event(
                                app,
                                PipelineEvent::DraftStreamDone {
                                    task_id: task_id.to_string(),
                                    step_id,
                                    target_path: target_path.to_string(),
                                    success: false,
                                    error: Some(err_msg.clone()),
                                },
                            );
                            return Err(err_msg);
                        }
                        if let Some(tok) = val.get("response").and_then(|v| v.as_str()) {
                            if !tok.is_empty() {
                                accumulated.push_str(tok);
                                emit_pipeline_event(
                                    app,
                                    PipelineEvent::DraftStreamChunk {
                                        task_id: task_id.to_string(),
                                        step_id,
                                        title: title.to_string(),
                                        action_class: action_class_str.to_string(),
                                        target_path: target_path.to_string(),
                                        model: model.clone(),
                                        chunk: tok.to_string(),
                                        accumulated: accumulated.clone(),
                                    },
                                );
                            }
                        }
                        if val.get("done").and_then(|v| v.as_bool()) == Some(true) {
                            saw_done = true;
                        }
                    }
                }
            }
            Ok(Ok(None)) => {
                // Process any trailing line without newline
                let trimmed = line_buf.trim();
                if !trimmed.is_empty() {
                    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                        if let Some(tok) = val.get("response").and_then(|v| v.as_str()) {
                            accumulated.push_str(tok);
                        }
                        if val.get("done").and_then(|v| v.as_bool()) == Some(true) {
                            saw_done = true;
                        }
                    }
                }
                break;
            }
            Ok(Err(e)) => {
                let err_msg = if e.is_connect() {
                    format!("Local Ollama connection lost at {url} (connection_refused) — service disconnected mid-stream.")
                } else if e.is_timeout() {
                    format!(
                        "Local generation canceled: exceeded total timeout of {}s — server was still generating (client disconnect). Raise local_total_timeout_secs or simplify the step. (client_timeout_total)",
                        cfg.local_total_timeout_secs
                    )
                } else {
                    format!(
                        "Local Ollama connection interrupted mid-stream on `{target_path}` (stream_interrupted): {e}"
                    )
                };
                emit_pipeline_event(
                    app,
                    PipelineEvent::DraftStreamDone {
                        task_id: task_id.to_string(),
                        step_id,
                        target_path: target_path.to_string(),
                        success: false,
                        error: Some(err_msg.clone()),
                    },
                );
                return Err(err_msg);
            }
        }
    }

    if !saw_done && accumulated.trim().is_empty() {
        let err_msg = format!(
            "Local Ollama stream closed prematurely before completing draft for `{target_path}` (no files written)."
        );
        emit_pipeline_event(
            app,
            PipelineEvent::DraftStreamDone {
                task_id: task_id.to_string(),
                step_id,
                target_path: target_path.to_string(),
                success: false,
                error: Some(err_msg.clone()),
            },
        );
        return Err(err_msg);
    }

    emit_pipeline_event(
        app,
        PipelineEvent::DraftStreamDone {
            task_id: task_id.to_string(),
            step_id,
            target_path: target_path.to_string(),
            success: true,
            error: None,
        },
    );

    Ok((model, accumulated))
}

/// Reads 1-indexed numbered lines (`L{i}: {line}`) from a file (`ollama_delegate_mcp.py` L169-L181).
pub fn read_numbered_lines(path: &Path, start_line: usize, end_line: usize) -> Result<String, String> {
    if path.is_dir() {
        return Err(format!(
            "Target '{}' is a directory, not a file. File-reading tools require a regular file.",
            path.display()
        ));
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {e}", path.display()))?;
    let lines: Vec<&str> = raw.lines().collect();
    let start = start_line.max(1);
    let end = end_line.min(lines.len());
    if start > end || lines.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    for idx in start..=end {
        out.push_str(&format!("L{}: {}\n", idx, lines[idx - 1]));
    }
    Ok(out)
}

/// Collects a shallow list of relative file paths (names ONLY, never file contents!) inside the active project
/// so the cloud interpreter knows what files exist without ever receiving raw file dumps (§1).
fn list_project_relative_files(project_root: &Path, max_files: usize) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &Path, cur: &Path, out: &mut Vec<String>, max_files: usize, depth: usize) {
        if depth > 3 || out.len() >= max_files {
            return;
        }
        let Ok(entries) = fs::read_dir(cur) else {
            return;
        };
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            if out.len() >= max_files {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | "__pycache__" | "venv" | ".git"
                )
            {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out, max_files, depth + 1);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    walk(project_root, project_root, &mut out, max_files, 0);
    out
}

/// Extracts JSON object from model response even if wrapped in markdown fences.
fn extract_json_object(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

/// Strips outer markdown code fences (` ```lang ... ``` `) from local `draft_code` output.
fn strip_code_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(after_open) = trimmed.strip_prefix("```") {
        if let Some(newline_idx) = after_open.find('\n') {
            let body = &after_open[newline_idx + 1..];
            if let Some(clean) = body.trim_end().strip_suffix("```") {
                return format!("{}\n", clean.trim_end());
            }
        }
    }
    format!("{}\n", trimmed)
}

/// Computes a unified diff between `old_text` and `new_text` for `target_path`.
pub fn make_unified_diff(target_path: &str, old_text: &str, new_text: &str) -> String {
    let diff = TextDiff::from_lines(old_text, new_text);
    diff.unified_diff()
        .context_radius(3)
        .header(&format!("a/{target_path}"), &format!("b/{target_path}"))
        .to_string()
}

/// Computes removal ratio and inconsistency note for a proposed diff (§3 Slice 2.3).
/// Returns (removal_ratio, caution_chip, inconsistency_note).
pub fn evaluate_destructive_diff(
    old_content: &str,
    unified_diff: &str,
    step_title: &str,
    step_instruction: &str,
    step_intent: Option<&str>,
    threshold: f64,
) -> (Option<f64>, Option<String>, Option<String>) {
    let old_lines_count = old_content.lines().count();
    let mut lines_removed = 0;
    for line in unified_diff.lines() {
        if line.starts_with('-') && !line.starts_with("---") {
            lines_removed += 1;
        }
    }
    let removal_ratio = if old_lines_count > 0 {
        lines_removed as f64 / old_lines_count as f64
    } else {
        0.0
    };

    let is_destructive = removal_ratio >= threshold && step_intent != Some("rewrite");

    if is_destructive {
        let pct = (removal_ratio * 100.0).round() as u64;
        let chip = format!("CAUTION: LARGE-SCALE DELETION ({pct}% of file)");
        let lower_check = format!("{} {}", step_title, step_instruction).to_lowercase();
        let targeted_keywords = [
            "targeted",
            "surrounding context",
            "replacement lines",
            "contextual edit",
            "small edit",
            "fix section",
            "fix typo",
            "corrected code section",
            "patch",
            "replacement section",
            "show replacement",
        ];
        let inconsistency_note = if targeted_keywords.iter().any(|k| lower_check.contains(k)) {
            Some(format!(
                "Step describes a targeted edit but the diff removes {pct}% of the file."
            ))
        } else {
            None
        };
        (Some(removal_ratio), Some(chip), inconsistency_note)
    } else {
        (Some(removal_ratio), None, None)
    }
}

/// Executes a shell command with timeout inside `cwd`.
pub async fn run_shell_command_bounded(
    cwd: &Path,
    cmd: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let child_fut = AsyncCommand::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output();

    match timeout(Duration::from_secs(timeout_secs.max(5)), child_fut).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status_code = output.status.code().unwrap_or(-1);
            let combined = format!(
                "Exit code: {status_code}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                stdout.trim(),
                stderr.trim()
            );
            if output.status.success() {
                Ok(combined)
            } else {
                Err(combined)
            }
        }
        Ok(Err(e)) => Err(format!("Failed to spawn shell command: {e}")),
        Err(_) => Err(format!(
            "Command timed out after {} seconds",
            timeout_secs.max(5)
        )),
    }
}

/// Validates a structured TaskPlan produced by the interpreter (§1.3 Slice 2.3, §2 & §3 Slice 2.4).
/// Ensures that:
/// 1. Empty/dot target paths are rejected unless a valid memory reference is delivered.
/// 2. File-reading tools targeting directories are rejected at parse time.
/// 3. Non-existent targets are allowed only if a prior step in the plan creates them (deferred validation).
/// 4. Drafting steps that require external facts (e.g., Framework 13 specs) have a preceding REQUEST step.
pub fn validate_plan(plan: &TaskPlan, project_root: Option<&Path>) -> Result<(), String> {
    for step in &plan.steps {
        let norm_target = step.target_path.trim().trim_start_matches("./");

        // 1. Empty/dot target check
        if (step.route_tool == "summarize_file" || step.action_class == ActionClass::Read)
            && (norm_target.is_empty() || norm_target == ".")
        {
            let has_memory_ref = !step.memories_used.is_empty()
                || step.instruction.contains('@')
                || step.title.contains('@')
                || step.title.to_lowercase().contains("memory")
                || !plan.memories_used.is_empty();

            if !has_memory_ref {
                return Err(format!(
                    "Plan rejected: Step #{} ('{}') specifies route_tool '{}' with empty target path and no memory reference.",
                    step.id, step.title, step.route_tool
                ));
            }
        }

        // 1b. REQUEST target and phrasing rules (§3.1 Slice 2.6)
        if step.action_class == ActionClass::Request || step.route_tool == "request" {
            if norm_target == "." || step.target_path.trim() == "." || step.target_path.trim() == "./" {
                return Err(format!(
                    "Plan rejected: Step #{} REQUEST step has target '{}'. REQUEST cards have no file target; target_path must be omitted or empty.",
                    step.id, step.target_path
                ));
            }

            if step.request_kind.as_deref() == Some("information") {
                let q_opt = step.question.as_deref().or(if step.instruction.contains('?') {
                    Some(step.instruction.as_str())
                } else {
                    None
                });

                if let Some(q) = q_opt {
                    if !q.trim().ends_with('?') {
                        return Err(format!(
                            "Plan rejected: Step #{} REQUEST (information) step question must end with '?'. Found: '{q}'",
                            step.id
                        ));
                    }
                } else {
                    return Err(format!(
                        "Plan rejected: Step #{} REQUEST (information) step must contain exactly ONE concrete question ending with '?'. Found: '{}'",
                        step.id, step.title
                    ));
                }

                if step.answer_format.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(format!(
                        "Plan rejected: Step #{} REQUEST (information) step must provide an answer_format hint (e.g. 'free text / comma-separated list / short spec block').",
                        step.id
                    ));
                }
            }
        }

        // 2. Directory vs. file validation at parse time (§3 Slice 2.4)
        if let Some(root) = project_root {
            let is_memory_retrieval = (norm_target.is_empty() || norm_target == ".")
                && (!step.memories_used.is_empty() || !plan.memories_used.is_empty());

            if !is_memory_retrieval
                && (step.route_tool == "summarize_file"
                    || (step.action_class == ActionClass::Read
                        && step.route_tool != "search_and_summarize"
                        && step.route_tool != "shell"))
            {
                let target_path = root.join(&step.target_path);
                if target_path.exists() {
                    if target_path.is_dir() {
                        return Err(format!(
                            "Plan rejected: Step #{} target '{}' is a directory, not a file. File-reading tools require a regular file.",
                            step.id, step.target_path
                        ));
                    }
                } else {
                    // Check if a prior step in the plan creates it (deferred validation)
                    let created_by_prior = plan.steps.iter().any(|prior| {
                        prior.id < step.id
                            && prior.target_path == step.target_path
                            && (prior.action_class == ActionClass::Create || prior.route_tool == "draft_code")
                    });
                    if !created_by_prior {
                        return Err(format!(
                            "Plan rejected: Step #{} target '{}' does not exist.",
                            step.id, step.target_path
                        ));
                    }
                }
            }
        }
    }

    // 3. REQUEST Mandate check (§2 Slice 2.4)
    for step in &plan.steps {
        if step.route_tool == "draft_code"
            || step.action_class == ActionClass::Create
            || step.action_class == ActionClass::Overwrite
        {
            let lower_inst = step.instruction.to_lowercase();
            let lower_title = step.title.to_lowercase();
            let lower_goal = plan.goal.to_lowercase();
            let mentions_external_specs = (lower_inst.contains("framework 13")
                || lower_title.contains("framework 13")
                || lower_goal.contains("framework 13")
                || lower_inst.contains("external spec")
                || lower_title.contains("external spec"))
                && !plan.memories_used.iter().any(|m| m.contains("framework"));

            if mentions_external_specs {
                let has_prior_request = plan.steps.iter().any(|prior| {
                    prior.id < step.id
                        && prior.action_class == ActionClass::Request
                        && prior.request_kind.as_deref() == Some("information")
                });
                if !has_prior_request {
                    return Err(format!(
                        "Plan rejected: Step #{} ('{}') drafts from external facts without a preceding REQUEST (information) step. Drafting from assumed values is forbidden per REQUEST mandate.",
                        step.id, step.title
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Builds the ordered list of cloud providers for cascade failover.
/// §2.2: Local-interpret and defer are NOT in the cascade — they remain manual Choice Card decisions.
pub fn build_cascade_provider_list(cfg: &AppConfig) -> Vec<(String, StageProvider)> {
    let mut list = Vec::new();
    let order = if cfg.cascade.order.is_empty() {
        let mut def = vec!["primary".to_string()];
        for alt in &cfg.alt_provider {
            def.push(alt.name.clone());
        }
        def
    } else {
        cfg.cascade.order.clone()
    };

    for item in order {
        let trimmed = item.trim();
        let lower = trimmed.to_ascii_lowercase();
        // Skip local and defer per §2.2
        if lower == "local" || lower == "defer" || lower.is_empty() {
            continue;
        }
        if lower == "primary" || lower == "lumo" || lower == "default" {
            list.push(("primary".to_string(), StageProvider::DefaultCloud));
        } else if let Some(alt) = cfg.alt_provider.iter().find(|a| a.name.eq_ignore_ascii_case(trimmed)) {
            list.push((alt.name.clone(), StageProvider::Alternate(alt.clone())));
        }
    }

    if list.is_empty() {
        list.push(("primary".to_string(), StageProvider::DefaultCloud));
    }
    list
}

/// Stage 1: Cloud Interpret (`prompt` → structured `TaskPlan`).
/// Hard rule (§1): sends ONLY the user prompt and relative file path names (never file contents!).
pub async fn run_interpret_stage(
    cfg: &AppConfig,
    project_root: &Path,
    prompt: &str,
) -> Result<TaskPlan, String> {
    run_interpret_stage_with_provider(cfg, project_root, prompt, &StageProvider::DefaultCloud).await
}

pub async fn run_interpret_stage_with_provider(
    cfg: &AppConfig,
    project_root: &Path,
    prompt: &str,
    provider: &StageProvider,
) -> Result<TaskPlan, String> {
    let file_names = list_project_relative_files(project_root, 60);
    let file_list_summary = if file_names.is_empty() {
        "(empty directory)".to_string()
    } else {
        file_names.join(", ")
    };

    let pipeline_generated = get_pipeline_generated_files(project_root);
    let gen_files_summary = if pipeline_generated.is_empty() {
        "[]".to_string()
    } else {
        format!("[\"{}\"]", pipeline_generated.join("\", \""))
    };

    let all_memories = load_all_memories().unwrap_or_default();
    let (_matched_memories, explicit_refs) = match_memories(&all_memories, prompt, project_root);
    let memory_catalog = build_cloud_memory_catalog(&all_memories);

    let mut expanded_cloud_memories = String::new();
    for mem_id in &explicit_refs {
        if let Some(mem) = all_memories.iter().find(|m| &m.id == mem_id) {
            if mem.cloud_share {
                expanded_cloud_memories.push_str(&format!(
                    "\n--- Explicit Memory [@{}] ---\n{}\n",
                    mem.id, mem.body
                ));
            } else {
                expanded_cloud_memories.push_str(&format!(
                    "\n--- Explicit Memory [@{}] ---\n(Memory marked local-only; available in local execution stages only)\n",
                    mem.id
                ));
            }
        }
    }

    let memory_section = if expanded_cloud_memories.is_empty() {
        format!("Memory Bank Catalog (~500 tokens max):\n{memory_catalog}")
    } else {
        format!(
            "Memory Bank Catalog (~500 tokens max):\n{memory_catalog}\n\nExplicitly Referenced Memories (Shareable):\n{expanded_cloud_memories}"
        )
    };

    let user_msg = format!(
        "User Task Request:\n{prompt}\n\nProject relative file names (names only, no contents):\n{file_list_summary}\n\npipeline_generated_files (unverified past outputs — DO NOT use as grounding sources):\n{gen_files_summary}\n\n{memory_section}"
    );

    let raw_reply = match provider {
        StageProvider::DefaultCloud => call_lumo_chat(cfg, "interpret", INTERPRET_SYSTEM_PROMPT, &user_msg).await?,
        StageProvider::LocalHeavy => call_local_stage_chat(cfg, "interpret", INTERPRET_SYSTEM_PROMPT, &user_msg).await?,
        StageProvider::Alternate(alt) => call_alt_provider_chat(alt, "interpret", INTERPRET_SYSTEM_PROMPT, &user_msg).await?,
    };
    let json_slice = extract_json_object(&raw_reply);
    let plan: TaskPlan = serde_json::from_str(json_slice).map_err(|e| {
        format!(
            "Interpreter returned invalid plan JSON ({e}). Raw response:\n{raw_reply}"
        )
    })?;
    validate_plan(&plan, Some(project_root))?;
    Ok(plan)
}

/// Stage 2: Local Execution + Safety Enforcer across all planned steps.
pub async fn run_execute_stage(
    cfg: &AppConfig,
    project_root: &Path,
    task_id: &str,
    plan: &TaskPlan,
    cruise_control: bool,
) -> Result<(Vec<StepExecutionLog>, Vec<ActionCard>), String> {
    run_execute_stage_with_events(None, cfg, project_root, task_id, plan, cruise_control).await
}

/// Stage 2 with optional Tauri `AppHandle` for live event ticker & streaming local drafts (§2.2 & §2.3 Slice 2).
pub async fn run_execute_stage_with_events(
    app: Option<&tauri::AppHandle>,
    cfg: &AppConfig,
    project_root: &Path,
    task_id: &str,
    plan: &TaskPlan,
    cruise_control: bool,
) -> Result<(Vec<StepExecutionLog>, Vec<ActionCard>), String> {
    let mut step_logs = Vec::new();
    let mut action_cards = Vec::new();
    let mut accumulated_local_context = String::new();
    let pipeline_generated = get_pipeline_generated_files(project_root);

    // Inject matched local memories into accumulated local context for local Ollama models (§1.2 Slice 2.1)
    let all_memories = load_all_memories().unwrap_or_default();
    let (matched_memories, _) = match_memories(&all_memories, &plan.goal, project_root);
    let local_mem_context = build_local_memory_context(&matched_memories);
    if !local_mem_context.is_empty() {
        accumulated_local_context.push_str(&format!("\n{local_mem_context}\n"));
    }

    for step in &plan.steps {
        if let Some(app_handle) = app {
            use tauri::Manager;
            if let Some(state) = app_handle.try_state::<crate::AppState>() {
                if state.queue.get_items().iter().any(|i| i.task_id.as_deref() == Some(task_id) && i.status == crate::queue::QueueItemStatus::Cancelled) {
                    emit_ticker_log(app, task_id, "execute", "warn", "Task execution stopped by user.");
                    return Err("Task execution stopped by user.".to_string());
                }
            }
        }
        let norm_target = step.target_path.trim_start_matches("./");
        let is_read_tool = matches!(
            step.route_tool.as_str(),
            "summarize_file" | "search_and_summarize" | "review_git_diff"
        );
        let is_pipeline_gen = is_read_tool && pipeline_generated.iter().any(|p| p == norm_target);
        let warning_chip = if is_pipeline_gen {
            emit_ticker_log(
                app,
                task_id,
                "execute",
                "warn",
                format!(
                    "step #{} reads pipeline-generated file `{}` (SOURCE: PIPELINE-GENERATED (UNVERIFIED))",
                    step.id, step.target_path
                ),
            );
            Some("SOURCE: PIPELINE-GENERATED (UNVERIFIED)".to_string())
        } else {
            None
        };
        let named_in_plan = plan
            .files_involved
            .iter()
            .any(|f| f == &step.target_path)
            || !step.target_path.is_empty();

        // Handle REQUEST action class (§2 Slice 2.3)
        if step.action_class == ActionClass::Request || step.route_tool == "request" {
            let req_kind = step
                .request_kind
                .clone()
                .unwrap_or_else(|| "information".to_string());

            // 1. Check deny list primacy for capability requests (§2.3)
            let is_denied = if req_kind == "capability" {
                let cmd_check = if !step.instruction.is_empty() {
                    check_command_deny_list(&step.instruction).err()
                } else {
                    None
                };
                let path_check = if !step.target_path.is_empty() {
                    check_path_deny_list(Path::new(&step.target_path)).err()
                } else {
                    None
                };
                cmd_check.or(path_check)
            } else {
                None
            };

            if let Some(deny_reason) = is_denied {
                emit_ticker_log(
                    app,
                    task_id,
                    "execute",
                    "bad",
                    format!(
                        "step #{} capability request auto-declined by §4 deny list: {deny_reason}",
                        step.id
                    ),
                );
                log_decision(
                    task_id,
                    step.id,
                    ActionClass::Request,
                    &step.target_path,
                    &step.title,
                    &project_root.to_string_lossy(),
                    GrantLevel::Read,
                    cruise_control,
                    cfg.dry_run,
                    "request_declined",
                    &format!("Auto-declined by §4 Deny List: {deny_reason}"),
                );
                action_cards.push(ActionCard {
                    card_id: format!("req-{}-{}", task_id, step.id),
                    step_id: step.id,
                    title: step.title.clone(),
                    action_class: ActionClass::Request,
                    target_path: step.target_path.clone(),
                    directory: project_root.to_string_lossy().to_string(),
                    grant_level: "READ".to_string(),
                    diff_or_command: format!(
                        "Capability Request: {}\nTarget: {}\nJustification: {}\nIf declined: {}",
                        step.title,
                        step.target_path,
                        step.justification.as_deref().unwrap_or("None stated"),
                        step.consequences_of_decline.as_deref().unwrap_or("None stated")
                    ),
                    proposed_content: None,
                    shell_command: None,
                    status: "auto_declined".to_string(),
                    decision_reason: format!("Auto-declined by §4 Deny List: {deny_reason}"),
                    execution_output: None,
                    warning_chip: Some("AUTO-DECLINED (DENY LIST)".to_string()),
                    request_kind: Some(req_kind),
                    justification: step.justification.clone(),
                    consequences_of_decline: step.consequences_of_decline.clone(),
                    user_provided_info: None,
                    removal_ratio: None,
                    inconsistency_note: None,
                    memory_delivery_chip: None,
                    question: None,
                    answer_format: None,
                    suggested_default: None,
                });
                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "enforcer-request".to_string(),
                    model: "safety-enforcer".to_string(),
                    route_tool: "request".to_string(),
                    action_class: ActionClass::Request,
                    target_path: step.target_path.clone(),
                    summary: format!("Capability request auto-declined by deny list: {deny_reason}"),
                    status: "declined".to_string(),
                    warning_chip: Some("AUTO-DECLINED (DENY LIST)".to_string()),
                    memory_delivery_chip: None,
                });
                continue;
            }

            // Normal pending request card
            emit_ticker_log(
                app,
                task_id,
                "execute",
                "warn",
                format!("step #{} requires user {}: '{}'", step.id, req_kind, step.title),
            );
            action_cards.push(ActionCard {
                card_id: format!("req-{}-{}", task_id, step.id),
                step_id: step.id,
                title: step.title.clone(),
                action_class: ActionClass::Request,
                target_path: step.target_path.clone(),
                directory: project_root.to_string_lossy().to_string(),
                grant_level: "READ".to_string(),
                diff_or_command: if req_kind == "information" {
                    let q = step.question.as_deref().or(Some(&step.instruction)).unwrap_or("");
                    format!(
                        "Question: {}\nFormat: {}\nSuggested: {}\nJustification: {}\nConsequences if declined: {}",
                        q,
                        step.answer_format.as_deref().unwrap_or("free text"),
                        step.suggested_default.as_deref().unwrap_or("none"),
                        step.justification.as_deref().unwrap_or("None stated"),
                        step.consequences_of_decline.as_deref().unwrap_or("None stated")
                    )
                } else {
                    format!(
                        "Request Kind: {}\nTitle: {}\nTarget: {}\nJustification: {}\nConsequences if declined: {}",
                        req_kind,
                        step.title,
                        step.target_path,
                        step.justification.as_deref().unwrap_or("None stated"),
                        step.consequences_of_decline.as_deref().unwrap_or("None stated")
                    )
                },
                proposed_content: None,
                shell_command: None,
                status: "pending_approval".to_string(),
                decision_reason: "Awaiting user resolution in Approval Queue".to_string(),
                execution_output: None,
                warning_chip: None,
                request_kind: Some(req_kind),
                justification: step.justification.clone(),
                consequences_of_decline: step.consequences_of_decline.clone(),
                user_provided_info: None,
                removal_ratio: None,
                inconsistency_note: None,
                memory_delivery_chip: None,
                question: step.question.clone().or_else(|| {
                    if step.instruction.contains('?') {
                        Some(step.instruction.clone())
                    } else {
                        None
                    }
                }),
                answer_format: step.answer_format.clone(),
                suggested_default: step.suggested_default.clone(),
            });
            step_logs.push(StepExecutionLog {
                step_id: step.id,
                title: step.title.clone(),
                tier: "user-request".to_string(),
                model: "user-interaction".to_string(),
                route_tool: "request".to_string(),
                action_class: ActionClass::Request,
                target_path: step.target_path.clone(),
                summary: format!("Request awaiting user input ({})", step.title),
                status: "pending_approval".to_string(),
                warning_chip: None,
                memory_delivery_chip: None,
            });
            continue;
        }

        // Determine runtime ActionClass and command string
        let (actual_class, cmd_opt): (ActionClass, Option<&str>) = match step.route_tool.as_str() {
            "summarize_file" | "search_and_summarize" | "digest_log_or_errors" | "review_git_diff" => {
                (ActionClass::Read, None)
            }
            "mkdir" => (ActionClass::Mkdir, None),
            "shell" => {
                if step.action_class == ActionClass::Read && is_readonly_command(&step.instruction) {
                    (ActionClass::Read, Some(step.instruction.as_str()))
                } else {
                    (ActionClass::Execute, Some(step.instruction.as_str()))
                }
            }
            "draft_code" => {
                let candidate = project_root.join(&step.target_path);
                if candidate.exists() {
                    (ActionClass::Overwrite, None)
                } else {
                    (ActionClass::Create, None)
                }
            }
            _ => (step.action_class, None),
        };

        // For draft_code, if the cloud model declared "create" on an existing file OR "overwrite" on a non-existing file:
        // note that if declared == Overwrite and file doesn't exist yet, we align declared to Create if step is draft_code,
        // EXCEPT when declared == Create and file ALREADY exists on disk (which §8.5 specifically tests!).
        let declared_for_enforcer = if step.route_tool == "draft_code"
            && step.action_class == ActionClass::Overwrite
            && actual_class == ActionClass::Create
        {
            ActionClass::Create
        } else if matches!(
            step.route_tool.as_str(),
            "summarize_file" | "search_and_summarize" | "digest_log_or_errors" | "review_git_diff"
        ) {
            ActionClass::Read
        } else if step.route_tool == "shell"
            && step.action_class == ActionClass::Read
            && actual_class == ActionClass::Execute
        {
            ActionClass::Execute
        } else {
            step.action_class
        };

        let decision = evaluate_action(
            project_root,
            &step.target_path,
            declared_for_enforcer,
            actual_class,
            cmd_opt,
            named_in_plan,
            &cfg.whitelist,
            cruise_control,
            cfg.dry_run,
        );

        // Handle Denied immediately before any local model execution or approval card
        if let EnforcementDecision::Denied { reason } = decision {
            emit_ticker_log(
                app,
                task_id,
                "execute",
                "bad",
                format!(
                    "step #{} BLOCKED by Enforcer ({}): {}",
                    step.id, step.target_path, reason
                ),
            );
            log_decision(
                task_id,
                step.id,
                actual_class,
                &step.target_path,
                &step.instruction,
                &project_root.to_string_lossy(),
                GrantLevel::Read,
                cruise_control,
                cfg.dry_run,
                "denied",
                &reason,
            );
            step_logs.push(StepExecutionLog {
                step_id: step.id,
                title: step.title.clone(),
                tier: "local-enforcer".to_string(),
                model: "kodiak-enforcer".to_string(),
                route_tool: step.route_tool.clone(),
                action_class: actual_class,
                target_path: step.target_path.clone(),
                summary: format!("BLOCKED: {reason}"),
                status: "denied".to_string(),
                warning_chip: warning_chip.clone(),
                memory_delivery_chip: None,
            });
            action_cards.push(ActionCard {
                card_id: Uuid::new_v4().to_string(),
                step_id: step.id,
                title: step.title.clone(),
                action_class: actual_class,
                target_path: step.target_path.clone(),
                directory: project_root.to_string_lossy().to_string(),
                grant_level: "DENY_LIST".to_string(),
                diff_or_command: step.instruction.clone(),
                proposed_content: None,
                shell_command: cmd_opt.map(|s| s.to_string()),
                status: "denied".to_string(),
                decision_reason: reason,
                execution_output: None,
                warning_chip: warning_chip.clone(),
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
            });
            continue;
        }

        // Emit live per-step model routing decision to ticker (§2.2 Slice 2)
        let route_desc = match step.route_tool.as_str() {
            "summarize_file" | "search_and_summarize" | "digest_log_or_errors" => {
                format!(
                    "step #{} → local-ollama/fast ({}) [{} on `{}`]",
                    step.id, cfg.fast_model, step.route_tool, step.target_path
                )
            }
            "review_git_diff" | "draft_code" => {
                format!(
                    "step #{} → local-ollama/coder ({}) [{} on `{}`]",
                    step.id, cfg.coder_model, step.route_tool, step.target_path
                )
            }
            "mkdir" => format!("step #{} → local-fs (native-fs) [mkdir `{}`]", step.id, step.target_path),
            _ => format!("step #{} → local-shell (native-shell) [`{}`]", step.id, step.instruction),
        };
        emit_ticker_log(app, task_id, "execute", "route", route_desc);

        let (step_memories, step_mem_payload) = resolve_step_memories(
            &all_memories,
            &step.memories_used,
            &step.title,
            &step.instruction,
            &plan.memories_used,
            cfg.memory_step_budget_chars,
        );

        let memory_chip = if !step_memories.is_empty() && !step_mem_payload.is_empty() {
            let slugs: Vec<String> = step_memories.iter().map(|m| m.id.clone()).collect();
            Some(format!(
                "memory: {} ({} chars delivered)",
                slugs.join(", "),
                step_mem_payload.len()
            ))
        } else if !step.memories_used.is_empty() {
            emit_ticker_log(
                app,
                task_id,
                "execute",
                "warn",
                format!(
                    "step #{} referenced memory {:?} resolved to empty payload",
                    step.id, step.memories_used
                ),
            );
            None
        } else {
            None
        };

        // Route step locally per §2 semantics
        match step.route_tool.as_str() {
            "summarize_file" => {
                let norm_target = step.target_path.trim().trim_start_matches("./");
                let is_memory_retrieval = (norm_target.is_empty() || norm_target == ".")
                    && !step_mem_payload.is_empty();

                let (used_model, summary) = if is_memory_retrieval {
                    emit_ticker_log(
                        app,
                        task_id,
                        "execute",
                        "info",
                        format!(
                            "step #{} delivering memory context directly into local pipeline...",
                            step.id
                        ),
                    );
                    let focus_line = if step.instruction.is_empty() {
                        String::new()
                    } else {
                        format!("\nFocus / Task: {}\n", step.instruction)
                    };
                    let prompt = format!(
                        "{SUMMARIZE_FILE_PROMPT_PREFIX}\n{focus_line}\n{SUMMARIZE_FILE_PROMPT_SUFFIX}\n\n{step_mem_payload}"
                    );
                    call_ollama_generate(cfg, &cfg.fast_model, &prompt, 0.1).await?
                } else {
                    let target = resolve_canonical_target(project_root, &step.target_path)?;
                    let start = step.start_line.unwrap_or(1);
                    let end = step.end_line.unwrap_or(1500);
                    let numbered = read_numbered_lines(&target, start, end).unwrap_or_else(|e| e);
                    let focus_line = if step.instruction.is_empty() {
                        String::new()
                    } else {
                        format!("\nPay special attention to: {}\n", step.instruction)
                    };
                    let mem_block = if !step_mem_payload.is_empty() {
                        format!("\n\nReferenced Memory Context:\n{step_mem_payload}")
                    } else {
                        String::new()
                    };
                    let prompt = format!(
                        "{SUMMARIZE_FILE_PROMPT_PREFIX}\n{focus_line}\n{SUMMARIZE_FILE_PROMPT_SUFFIX}\n\n{numbered}{mem_block}"
                    );
                    call_ollama_generate(cfg, &cfg.fast_model, &prompt, 0.1).await?
                };

                let target_desc = if norm_target.is_empty() || norm_target == "." {
                    "memory".to_string()
                } else {
                    step.target_path.clone()
                };
                accumulated_local_context.push_str(&format!(
                    "\n[Summary of {}]:\n{}\n",
                    target_desc, summary
                ));
                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-ollama (fast/3b)".to_string(),
                    model: used_model,
                    route_tool: "summarize_file".to_string(),
                    action_class: ActionClass::Read,
                    target_path: step.target_path.clone(),
                    summary,
                    status: "completed".to_string(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });
            }

            "search_and_summarize" => {
                let pattern = step
                    .search_pattern
                    .clone()
                    .unwrap_or_else(|| step.instruction.clone());
                let search_dir = resolve_canonical_target(project_root, &step.target_path)?;
                let rg_out = Command::new("rg")
                    .args([
                        "-n",
                        "--no-heading",
                        "--color=never",
                        "-C",
                        "1",
                        &pattern,
                        &search_dir.to_string_lossy(),
                    ])
                    .output();

                let snippet = match rg_out {
                    Ok(o) => {
                        let text = String::from_utf8_lossy(&o.stdout);
                        let lines: Vec<&str> = text.lines().take(250).collect();
                        if lines.is_empty() {
                            format!("No matches found for pattern '{pattern}'.")
                        } else {
                            lines.join("\n")
                        }
                    }
                    Err(_) => "rg execution skipped or no matches.".to_string(),
                };

                let mem_block = if !step_mem_payload.is_empty() {
                    format!("\n\nReferenced Memory Context:\n{step_mem_payload}")
                } else {
                    String::new()
                };
                let prompt = format!(
                    "Below are code search matches for `{pattern}` in `{}`.\nTask: {}\n{SEARCH_SUMMARIZE_PROMPT_INSTRUCTION}\n\n{snippet}{mem_block}",
                    step.target_path, step.instruction
                );
                let (used_model, summary) =
                    call_ollama_generate(cfg, &cfg.fast_model, &prompt, 0.1).await?;
                accumulated_local_context.push_str(&format!(
                    "\n[Search matches for '{pattern}']:\n{summary}\n"
                ));
                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-ollama (fast/3b)".to_string(),
                    model: used_model,
                    route_tool: "search_and_summarize".to_string(),
                    action_class: ActionClass::Read,
                    target_path: step.target_path.clone(),
                    summary,
                    status: "completed".to_string(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });
            }

            "digest_log_or_errors" => {
                let target = resolve_canonical_target(project_root, &step.target_path)?;
                let raw = fs::read_to_string(&target).unwrap_or_default();
                let tail: Vec<&str> = raw.lines().rev().take(400).collect();
                let content = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                let mem_block = if !step_mem_payload.is_empty() {
                    format!("\n\nReferenced Memory Context:\n{step_mem_payload}")
                } else {
                    String::new()
                };
                let prompt = format!("{DIGEST_LOG_PROMPT_PREFIX}\n\n{content}{mem_block}");
                let (used_model, summary) =
                    call_ollama_generate(cfg, &cfg.fast_model, &prompt, 0.1).await?;
                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-ollama (fast/3b)".to_string(),
                    model: used_model,
                    route_tool: "digest_log_or_errors".to_string(),
                    action_class: ActionClass::Read,
                    target_path: step.target_path.clone(),
                    summary,
                    status: "completed".to_string(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });
            }

            "review_git_diff" => {
                let out = Command::new("git")
                    .args(["-C", &project_root.to_string_lossy(), "diff"])
                    .output()
                    .map_err(|e| format!("Failed to run git diff: {e}"))?;
                let mut diff_text = String::from_utf8_lossy(&out.stdout).to_string();
                if diff_text.len() > 36000 {
                    diff_text.truncate(36000);
                    diff_text.push_str("\n... [diff truncated]");
                }
                let (used_model, summary) = if diff_text.trim().is_empty() {
                    (cfg.coder_model.clone(), "No git diff changes found.".to_string())
                } else {
                    let mem_block = if !step_mem_payload.is_empty() {
                        format!("\n\nReferenced Memory Context:\n{step_mem_payload}")
                    } else {
                        String::new()
                    };
                    let prompt = format!(
                        "{REVIEW_GIT_DIFF_PROMPT_PREFIX}\n\n```diff\n{diff_text}\n```{mem_block}"
                    );
                    call_ollama_generate(cfg, &cfg.coder_model, &prompt, 0.1).await?
                };
                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-ollama (coder/7b)".to_string(),
                    model: used_model,
                    route_tool: "review_git_diff".to_string(),
                    action_class: ActionClass::Read,
                    target_path: step.target_path.clone(),
                    summary,
                    status: "completed".to_string(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });
            }

            "draft_code" => {
                let target = resolve_canonical_target(project_root, &step.target_path)?;
                let old_content = if target.exists() {
                    fs::read_to_string(&target).unwrap_or_default()
                } else {
                    String::new()
                };
                let numbered_ctx = if target.exists() {
                    let start = step.start_line.unwrap_or(1);
                    let end = step.end_line.unwrap_or(1500);
                    format!(
                        "\n\nExisting file `{}` (numbered lines):\n{}",
                        step.target_path,
                        read_numbered_lines(&target, start, end).unwrap_or_default()
                    )
                } else {
                    format!("\n\nTarget file `{}` is a new file.", step.target_path)
                };

                let extra_ctx = if accumulated_local_context.is_empty() {
                    String::new()
                } else {
                    format!("\n\nPrior step summaries:\n{accumulated_local_context}")
                };

                let mem_block = if !step_mem_payload.is_empty() {
                    format!("\n\nReferenced Memory Context:\n{step_mem_payload}")
                } else {
                    String::new()
                };

                let is_doc = step.target_path.ends_with(".md")
                    || step.target_path.ends_with(".markdown")
                    || step.target_path.ends_with(".txt");
                let prefix = if is_doc {
                    DRAFT_DOC_PROMPT_PREFIX
                } else {
                    DRAFT_CODE_PROMPT_PREFIX
                };

                let prompt = format!(
                    "{prefix}\n\nSpecification:\n{}{}{}{}",
                    step.instruction, mem_block, numbered_ctx, extra_ctx
                );
                emit_ticker_log(
                    app,
                    task_id,
                    "execute",
                    "info",
                    format!(
                        "step #{} streaming draft_code for `{}` ({})...",
                        step.id, step.target_path, actual_class
                    ),
                );
                let (used_model, drafted_raw) = call_ollama_generate_streaming(
                    app,
                    cfg,
                    task_id,
                    step.id,
                    &step.title,
                    &actual_class.to_string(),
                    &step.target_path,
                    &cfg.coder_model,
                    &prompt,
                    0.1,
                )
                .await?;
                let new_content = strip_code_fences(&drafted_raw);
                let unified_diff = make_unified_diff(&step.target_path, &old_content, &new_content);

                let (status_str, reason_str, grant_str, dir_str) = match &decision {
                    EnforcementDecision::AutoExecuteUnderGrant {
                        grant_level,
                        directory,
                        note,
                    } => {
                        if let Some(parent) = target.parent() {
                            if !parent.exists() && *grant_level >= GrantLevel::Build {
                                let _ = fs::create_dir_all(parent);
                            }
                        }
                        fs::write(&target, &new_content).map_err(|e| {
                            format!("Failed to write '{}': {e}", target.display())
                        })?;
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            "draft_code auto-write",
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "grant_auto_executed",
                            note,
                        );
                        (
                            "auto_executed_grant".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                        )
                    }
                    EnforcementDecision::DryRunSuppressed {
                        grant_level,
                        directory,
                        note,
                        ..
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            "draft_code dry-run",
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "dry_run_suppressed",
                            note,
                        );
                        (
                            "dry_run".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                        )
                    }
                    EnforcementDecision::NeedsApproval {
                        grant_level,
                        directory,
                        reason,
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            "draft_code queued for approval",
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "queued_for_approval",
                            reason,
                        );
                        (
                            "pending_approval".to_string(),
                            reason.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                        )
                    }
                    EnforcementDecision::Denied { reason } => (
                        "denied".to_string(),
                        reason.clone(),
                        "DENIED".to_string(),
                        project_root.to_string_lossy().to_string(),
                    ),
                };

                let (ratio_opt, destruction_chip, inconsistency_note) = evaluate_destructive_diff(
                    &old_content,
                    &unified_diff,
                    &step.title,
                    &step.instruction,
                    step.intent.as_deref(),
                    cfg.destructive_diff_threshold,
                );

                let effective_warning_chip = destruction_chip.or(warning_chip.clone());

                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-ollama (coder/7b)".to_string(),
                    model: used_model,
                    route_tool: "draft_code".to_string(),
                    action_class: actual_class,
                    target_path: step.target_path.clone(),
                    summary: format!(
                        "Drafted {} ({} lines diff) — {}",
                        step.target_path,
                        unified_diff.lines().count(),
                        status_str
                    ),
                    status: status_str.clone(),
                    warning_chip: effective_warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });

                action_cards.push(ActionCard {
                    card_id: Uuid::new_v4().to_string(),
                    step_id: step.id,
                    title: step.title.clone(),
                    action_class: actual_class,
                    target_path: step.target_path.clone(),
                    directory: dir_str,
                    grant_level: grant_str,
                    diff_or_command: unified_diff,
                    proposed_content: Some(new_content),
                    shell_command: None,
                    status: status_str,
                    decision_reason: reason_str,
                    execution_output: None,
                    warning_chip: effective_warning_chip,
                    request_kind: None,
                    justification: None,
                    consequences_of_decline: None,
                    user_provided_info: None,
                    removal_ratio: ratio_opt,
                    inconsistency_note,
                    memory_delivery_chip: memory_chip.clone(),
                    question: None,
                    answer_format: None,
                    suggested_default: None,
                });
            }

            "mkdir" => {
                let target = resolve_canonical_target(project_root, &step.target_path)?;
                let preview = format!("mkdir -p {}", step.target_path);
                let (status_str, reason_str, grant_str, dir_str, exec_out) = match &decision {
                    EnforcementDecision::AutoExecuteUnderGrant {
                        grant_level,
                        directory,
                        note,
                    } => {
                        fs::create_dir_all(&target).map_err(|e| {
                            format!("Failed to create directory '{}': {e}", target.display())
                        })?;
                        log_decision(
                            task_id,
                            step.id,
                            ActionClass::Mkdir,
                            &step.target_path,
                            &preview,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "grant_auto_executed",
                            note,
                        );
                        (
                            "auto_executed_grant".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            Some(format!("Created directory {}", target.display())),
                        )
                    }
                    EnforcementDecision::DryRunSuppressed {
                        grant_level,
                        directory,
                        note,
                        ..
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            ActionClass::Mkdir,
                            &step.target_path,
                            &preview,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "dry_run_suppressed",
                            note,
                        );
                        (
                            "dry_run".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            None,
                        )
                    }
                    EnforcementDecision::NeedsApproval {
                        grant_level,
                        directory,
                        reason,
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            ActionClass::Mkdir,
                            &step.target_path,
                            &preview,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "queued_for_approval",
                            reason,
                        );
                        (
                            "pending_approval".to_string(),
                            reason.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            None,
                        )
                    }
                    EnforcementDecision::Denied { reason } => (
                        "denied".to_string(),
                        reason.clone(),
                        "DENIED".to_string(),
                        project_root.to_string_lossy().to_string(),
                        None,
                    ),
                };

                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-fs".to_string(),
                    model: "native-fs".to_string(),
                    route_tool: "mkdir".to_string(),
                    action_class: ActionClass::Mkdir,
                    target_path: step.target_path.clone(),
                    summary: format!("{preview} ({status_str})"),
                    status: status_str.clone(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });

                action_cards.push(ActionCard {
                    card_id: Uuid::new_v4().to_string(),
                    step_id: step.id,
                    title: step.title.clone(),
                    action_class: ActionClass::Mkdir,
                    target_path: step.target_path.clone(),
                    directory: dir_str,
                    grant_level: grant_str,
                    diff_or_command: preview,
                    proposed_content: None,
                    shell_command: None,
                    status: status_str,
                    decision_reason: reason_str,
                    execution_output: exec_out,
                    warning_chip: warning_chip.clone(),
                    request_kind: None,
                    justification: None,
                    consequences_of_decline: None,
                    user_provided_info: None,
                    removal_ratio: None,
                    inconsistency_note: None,
                    memory_delivery_chip: memory_chip.clone(),
                    question: None,
                    answer_format: None,
                    suggested_default: None,
                });
            }

            "shell" | _ => {
                let cmd_str = step.instruction.clone();
                let (status_str, reason_str, grant_str, dir_str, exec_out) = match &decision {
                    EnforcementDecision::AutoExecuteUnderGrant {
                        grant_level,
                        directory,
                        note,
                    } => {
                        let out = match run_shell_command_bounded(
                            project_root,
                            &cmd_str,
                            cfg.command_timeout_secs,
                        )
                        .await
                        {
                            Ok(s) => s,
                            Err(e) => e,
                        };
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            &cmd_str,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "grant_auto_executed",
                            note,
                        );
                        (
                            "auto_executed_grant".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            Some(out),
                        )
                    }
                    EnforcementDecision::DryRunSuppressed {
                        grant_level,
                        directory,
                        note,
                        ..
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            &cmd_str,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "dry_run_suppressed",
                            note,
                        );
                        (
                            "dry_run".to_string(),
                            note.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            None,
                        )
                    }
                    EnforcementDecision::NeedsApproval {
                        grant_level,
                        directory,
                        reason,
                    } => {
                        log_decision(
                            task_id,
                            step.id,
                            actual_class,
                            &step.target_path,
                            &cmd_str,
                            directory,
                            *grant_level,
                            cruise_control,
                            cfg.dry_run,
                            "queued_for_approval",
                            reason,
                        );
                        (
                            "pending_approval".to_string(),
                            reason.clone(),
                            grant_level.to_string(),
                            directory.clone(),
                            None,
                        )
                    }
                    EnforcementDecision::Denied { reason } => (
                        "denied".to_string(),
                        reason.clone(),
                        "DENIED".to_string(),
                        project_root.to_string_lossy().to_string(),
                        None,
                    ),
                };

                step_logs.push(StepExecutionLog {
                    step_id: step.id,
                    title: step.title.clone(),
                    tier: "local-shell".to_string(),
                    model: "native-shell".to_string(),
                    route_tool: "shell".to_string(),
                    action_class: actual_class,
                    target_path: step.target_path.clone(),
                    summary: format!("Command `{cmd_str}` ({status_str})"),
                    status: status_str.clone(),
                    warning_chip: warning_chip.clone(),
                    memory_delivery_chip: memory_chip.clone(),
                });

                action_cards.push(ActionCard {
                    card_id: Uuid::new_v4().to_string(),
                    step_id: step.id,
                    title: step.title.clone(),
                    action_class: actual_class,
                    target_path: step.target_path.clone(),
                    directory: dir_str,
                    grant_level: grant_str,
                    diff_or_command: cmd_str.clone(),
                    proposed_content: None,
                    shell_command: Some(cmd_str),
                    status: status_str,
                    decision_reason: reason_str,
                    execution_output: exec_out,
                    warning_chip: warning_chip.clone(),
                    request_kind: None,
                    justification: None,
                    consequences_of_decline: None,
                    user_provided_info: None,
                    removal_ratio: None,
                    inconsistency_note: None,
                    memory_delivery_chip: memory_chip.clone(),
                    question: None,
                    answer_format: None,
                    suggested_default: None,
                });
            }
        }
    }

    Ok((step_logs, action_cards))
}

/// Stage 3: Cloud Verify (reviews ONLY user prompt, plan, condensed summaries, and diffs — §1 Hard Rule).
pub fn build_verify_user_message(
    prompt: &str,
    plan: &TaskPlan,
    step_logs: &[StepExecutionLog],
    action_cards: &[ActionCard],
    memory_catalog: &str,
) -> String {
    let plan_json = serde_json::to_string_pretty(plan).unwrap_or_default();

    let mut condensed_summaries = String::new();
    for log in step_logs {
        let mut short = log.summary.clone();
        if short.len() > 800 {
            short.truncate(800);
            short.push_str("...");
        }
        let mem_str = match &log.memory_delivery_chip {
            Some(chip) => format!(" [{chip}]"),
            None => String::new(),
        };
        condensed_summaries.push_str(&format!(
            "- Step {} ({} via {}{mem_str}): {}\n",
            log.step_id, log.route_tool, log.model, short
        ));
    }

    let mut diffs_payload = String::new();
    for card in action_cards {
        let mut diff_short = card.diff_or_command.clone();
        if diff_short.len() > 4000 {
            diff_short.truncate(4000);
            diff_short.push_str("\n... [diff truncated]");
        }
        diffs_payload.push_str(&format!(
            "\n--- Card [Step {} | {} | {} | status={}] ---\n{}\n",
            card.step_id, card.action_class, card.target_path, card.status, diff_short
        ));
    }

    let mut grounding_warnings = String::new();
    for log in step_logs {
        if let Some(chip) = &log.warning_chip {
            grounding_warnings.push_str(&format!(
                "- Step #{} ({}) targets pipeline-generated file `{}` [{}]. Scrutinize whether output is genuinely grounded in authoritative specs or propagating unverified past hallucinations.\n",
                log.step_id, log.route_tool, log.target_path, chip
            ));
        }
    }
    for card in action_cards {
        if let Some(mem_chip) = &card.memory_delivery_chip {
            grounding_warnings.push_str(&format!(
                "- Step #{} memory delivery: [{}].\n",
                card.step_id, mem_chip
            ));
        }
        if let Some(chip) = &card.warning_chip {
            if !grounding_warnings.contains(chip) {
                grounding_warnings.push_str(&format!(
                    "- Action card for Step #{} (`{}`) flagged: [{}]. Scrutinize proposed changes carefully.\n",
                    card.step_id, card.target_path, chip
                ));
            }
        }
        if let Some(note) = &card.inconsistency_note {
            grounding_warnings.push_str(&format!(
                "- Action card for Step #{} (`{}`) consistency warning: {}\n",
                card.step_id, card.target_path, note
            ));
        }
        if card.action_class == ActionClass::Request {
            if let Some(info) = &card.user_provided_info {
                let short_info = if info.len() > 100 { &info[..97] } else { info };
                grounding_warnings.push_str(&format!(
                    "- Step #{} requested info: '{}'. User provided: '{}'.\n",
                    card.step_id, card.title, short_info
                ));
            }
        }
    }
    let grounding_section = if grounding_warnings.is_empty() {
        String::new()
    } else {
        format!("\nGrounding & Epistemic Warnings:\n{grounding_warnings}")
    };

    format!(
        "Original User Prompt:\n{prompt}\n\nStructured Plan:\n{plan_json}\n\nMemory Catalog:\n{memory_catalog}\n\nCondensed Local Step Summaries:\n{condensed_summaries}\n\nDiffs & Proposed Actions:\n{diffs_payload}{grounding_section}"
    )
}

pub async fn run_verify_stage(
    cfg: &AppConfig,
    prompt: &str,
    plan: &TaskPlan,
    step_logs: &[StepExecutionLog],
    action_cards: &[ActionCard],
) -> Result<VerificationResult, String> {
    run_verify_stage_with_provider(cfg, prompt, plan, step_logs, action_cards, &StageProvider::DefaultCloud).await
}

pub async fn run_verify_stage_with_provider(
    cfg: &AppConfig,
    prompt: &str,
    plan: &TaskPlan,
    step_logs: &[StepExecutionLog],
    action_cards: &[ActionCard],
    provider: &StageProvider,
) -> Result<VerificationResult, String> {
    let all_memories = load_all_memories().unwrap_or_default();
    let memory_catalog = build_cloud_memory_catalog(&all_memories);
    let verify_user_msg = build_verify_user_message(prompt, plan, step_logs, action_cards, &memory_catalog);

    let raw_reply = match provider {
        StageProvider::DefaultCloud => call_lumo_chat(cfg, "verify", VERIFY_SYSTEM_PROMPT, &verify_user_msg).await?,
        StageProvider::LocalHeavy => call_local_stage_chat(cfg, "verify", VERIFY_SYSTEM_PROMPT, &verify_user_msg).await?,
        StageProvider::Alternate(alt) => call_alt_provider_chat(alt, "verify", VERIFY_SYSTEM_PROMPT, &verify_user_msg).await?,
    };
    let json_slice = extract_json_object(&raw_reply);
    if let Ok(parsed) = serde_json::from_str::<VerificationResult>(json_slice) {
        return Ok(parsed);
    }

    // Fallback if model returned plain text instead of JSON
    Ok(VerificationResult {
        verdict: "concerns".to_string(),
        rationale: raw_reply.trim().to_string(),
    })
}

pub fn build_posture_summary(
    cfg: &AppConfig,
    cruise_control: bool,
    action_cards: &[ActionCard],
) -> (String, usize, usize, usize) {
    let auto_count = action_cards
        .iter()
        .filter(|c| c.status == "auto_executed_grant")
        .count();
    let queued_count = action_cards
        .iter()
        .filter(|c| c.status == "pending_approval")
        .count();
    let denied_count = action_cards.iter().filter(|c| c.status == "denied").count();

    let posture = if cfg.dry_run {
        format!(
            "Dry-Run Mode (Zero Writes) — {} dirs in whitelist",
            cfg.whitelist.len()
        )
    } else if cruise_control {
        format!("Cruise: {} dirs granted", cfg.whitelist.len())
    } else {
        "Manual approval".to_string()
    };

    (posture, auto_count, queued_count, denied_count)
}

pub fn resolve_active_project_root(cfg: &AppConfig) -> Result<PathBuf, String> {
    let raw = cfg.active_project.as_deref().ok_or_else(|| {
        "No active project selected. Add a directory to the Project Whitelist in Settings and set it as active."
            .to_string()
    })?;
    let expanded = expand_tilde(raw);
    let canon = fs::canonicalize(&expanded).map_err(|e| {
        format!(
            "Active project directory '{}' does not exist or cannot be resolved: {e}",
            expanded.display()
        )
    })?;

    // Verify it is in the whitelist
    let in_whitelist = cfg
        .whitelist
        .iter()
        .any(|w| fs::canonicalize(expand_tilde(&w.path)).ok().as_ref() == Some(&canon));

    if !in_whitelist {
        return Err(format!(
            "Active project '{}' is not in the Project Whitelist. Add it in Settings first.",
            canon.display()
        ));
    }
    Ok(canon)
}

pub fn create_initial_task_record(
    cfg: &AppConfig,
    prompt: &str,
    project_root: &Path,
    cruise_control: bool,
) -> TaskRecord {
    let journal_path = journal_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~/.local/share/kodiak/journal.jsonl".to_string());
    let (posture_summary, _, _, _) = build_posture_summary(cfg, cruise_control, &[]);

    TaskRecord {
        task_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        title: prompt.trim().to_string(),
        starred: false,
        prompt: prompt.to_string(),
        project_dir: project_root.to_string_lossy().to_string(),
        cloud_model: cfg.cloud_model.clone(),
        cruise_control,
        dry_run: cfg.dry_run,
        posture_summary,
        auto_executed_count: 0,
        queued_count: 0,
        denied_count: 0,
        plan: None,
        step_logs: Vec::new(),
        action_cards: Vec::new(),
        verification: None,
        failed_stage: None,
        stage_error: None,
        journal_path,
        task_scoped_grants: Vec::new(),
        task_provided_info: Vec::new(),
        stage_error_kind: None,
        choice_cards: Vec::new(),
        deferred: false,
        deferred_stage: None,
        defer_reason: None,
        local_grade_plan: false,
        local_grade_verify: false,
        provider_interpret: None,
        provider_verify: None,
    }
}

pub fn save_and_return_record(record: TaskRecord) -> Result<TaskRecord, String> {
    let _ = upsert_task_record(&record);

    let mut providers_used = Vec::new();
    if let Some(ref p) = record.provider_interpret {
        providers_used.push(p.clone());
    }
    if let Some(ref p) = record.provider_verify {
        if !providers_used.contains(p) {
            providers_used.push(p.clone());
        }
    }
    let meta = crate::run_log::TaskRunMeta {
        task_id: record.task_id.clone(),
        prompt_hash: crate::run_log::compute_prompt_hash(&record.prompt),
        project: record.project_dir.clone(),
        providers_used,
        final_status: if record.failed_stage.is_some() {
            "FAILED".to_string()
        } else {
            "COMPLETED".to_string()
        },
        updated_at: Utc::now().to_rfc3339(),
    };
    let _ = crate::run_log::write_task_run_meta(&record.task_id, &meta);

    if let Ok(cfg) = crate::config::load_or_seed_config() {
        if let Ok(runs_dir) = crate::config::runs_dir_path() {
            let _ = crate::run_log::prune_run_logs(&runs_dir, cfg.run_log_keep_runs, cfg.run_log_max_mb);
        }
    }

    Ok(record)
}
