use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GrantLevel {
    Read = 0,
    Create = 1,
    Build = 2,
    Autonomous = 3,
}

impl Default for GrantLevel {
    fn default() -> Self {
        GrantLevel::Read
    }
}

impl std::fmt::Display for GrantLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantLevel::Read => write!(f, "READ"),
            GrantLevel::Create => write!(f, "CREATE"),
            GrantLevel::Build => write!(f, "BUILD"),
            GrantLevel::Autonomous => write!(f, "AUTONOMOUS"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WhitelistEntry {
    pub path: String,
    pub level: GrantLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AltProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

impl Default for AltProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            max_context_chars: None,
            tier: None,
            timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CascadeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub consent: bool,
    #[serde(default)]
    pub order: Vec<String>,
}

impl Default for CascadeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            consent: false,
            order: Vec::new(),
        }
    }
}

fn deserialize_cascade<'de, D>(deserializer: D) -> Result<CascadeConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CascadeOrVec {
        Single(CascadeConfig),
        List(Vec<CascadeConfig>),
    }
    match CascadeOrVec::deserialize(deserializer)? {
        CascadeOrVec::Single(c) => Ok(c),
        CascadeOrVec::List(mut list) => Ok(list.pop().unwrap_or_default()),
    }
}

fn default_run_log_level() -> String {
    "info".to_string()
}

fn default_run_log_max_mb() -> usize {
    50
}

fn default_run_log_keep_runs() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub lumo_url: String,
    pub lumo_api_key: String,
    pub cloud_model: String,
    #[serde(default, alias = "cloud_tier", skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    pub ollama_url: String,
    pub fast_model: String,
    pub coder_model: String,
    pub heavy_model: String,
    pub command_timeout_secs: u64,
    pub local_idle_timeout_secs: u64,
    pub local_total_timeout_secs: u64,
    pub memory_step_budget_chars: usize,
    pub memory_auto_sanitize: bool,
    pub destructive_diff_threshold: f64,
    pub dry_run: bool,
    pub wiki_dir: String,
    pub wiki_enabled: bool,
    pub wiki_configured: bool,
    pub projects_dir: String,
    pub active_project: Option<String>,
    pub whitelist: Vec<WhitelistEntry>,
    #[serde(default)]
    pub alt_provider: Vec<AltProviderConfig>,
    #[serde(default, deserialize_with = "deserialize_cascade")]
    pub cascade: CascadeConfig,
    #[serde(default = "default_run_log_level")]
    pub run_log_level: String,
    #[serde(default = "default_run_log_max_mb")]
    pub run_log_max_mb: usize,
    #[serde(default = "default_run_log_keep_runs")]
    pub run_log_keep_runs: usize,
    // New fields
    #[serde(default)]
    pub exclusion_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_css: Option<String>,
    #[serde(default)]
    pub ui_collapsed: std::collections::HashMap<String, bool>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            lumo_url: "http://127.0.0.1:3003/v1".to_string(),
            lumo_api_key: "frank-local-lumo-bridge".to_string(),
            cloud_model: "lumo".to_string(),
            tier: Some("medium".to_string()),
            ollama_url: "http://127.0.0.1:11434".to_string(),
            fast_model: "llama3.2:3b".to_string(),
            coder_model: "qwen2.5-coder:7b".to_string(),
            heavy_model: "qwen2.5-coder:14b".to_string(),
            command_timeout_secs: 60,
            local_idle_timeout_secs: 60,
            local_total_timeout_secs: 600,
            memory_step_budget_chars: 8000,
            memory_auto_sanitize: true,
            destructive_diff_threshold: 0.40,
            dry_run: false,
            #[cfg(windows)]
            wiki_dir: "ProjectsWiki".to_string(),
            #[cfg(not(windows))]
            wiki_dir: "/usr/share/kodiak/projectswiki".to_string(),
            wiki_enabled: true,
            wiki_configured: false,
            projects_dir: "~/Projects".to_string(),
            active_project: None,
            // §2.1 binding rule: seeds on first run with whitelist = [] (EMPTY)
            whitelist: Vec::new(),
            exclusion_list: Vec::new(),
            custom_css: None,
            ui_collapsed: std::collections::HashMap::new(),

            alt_provider: Vec::new(),
            cascade: CascadeConfig::default(),
            run_log_level: "info".to_string(),
            run_log_max_mb: 50,
            run_log_keep_runs: 20,
        }
    }
}

pub fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path_str == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path_str)
}

/// Resolves the ProjectsWiki root directory, prioritizing local repository folder `./ProjectsWiki`,
/// then explicitly configured `wiki_dir`, executable bundle resources, then system install `/usr/share/kodiak/ProjectsWiki`.
pub fn resolve_wiki_dir(cfg: &AppConfig) -> PathBuf {
    let configured = expand_tilde(&cfg.wiki_dir);
    let is_valid = |p: &Path| {
        p.exists()
            && (p.join("wiki.sh").exists()
                || p.join("mkdocs.yml").exists()
                || p.join("kodiak-projectswiki.exe").exists()
                || p.join("wiki_server.py").exists())
    };

    // 1. Explicitly configured path if it exists and is valid
    if is_valid(&configured) {
        return configured;
    }
    // 2. Relative to current working directory: ./ProjectsWiki
    let cwd_wiki = PathBuf::from("ProjectsWiki");
    if is_valid(&cwd_wiki) {
        if let Ok(canon) = cwd_wiki.canonicalize() {
            return canon;
        }
        return cwd_wiki;
    }
    // 3. Relative to current executable directory:
    //    exe_dir/resources/ProjectsWiki (Tauri bundle on Windows)
    //    exe_dir/ProjectsWiki
    //    exe_dir/../resources/ProjectsWiki
    //    exe_dir/../ProjectsWiki
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let res_wiki = exe_dir.join("resources").join("ProjectsWiki");
            if is_valid(&res_wiki) {
                return res_wiki;
            }
            let exe_wiki = exe_dir.join("ProjectsWiki");
            if is_valid(&exe_wiki) {
                return exe_wiki;
            }
            if let Some(parent) = exe_dir.parent() {
                let parent_res_wiki = parent.join("resources").join("ProjectsWiki");
                if is_valid(&parent_res_wiki) {
                    return parent_res_wiki;
                }
                let parent_wiki = parent.join("ProjectsWiki");
                if is_valid(&parent_wiki) {
                    return parent_wiki;
                }
            }
        }
    }
    // 4. Windows LocalAppData check
    #[cfg(windows)]
    if let Some(local_app_data) = dirs::data_local_dir() {
        let app_wiki = local_app_data.join("Programs").join("Kodiak").join("resources").join("ProjectsWiki");
        if is_valid(&app_wiki) {
            return app_wiki;
        }
        let direct_wiki = local_app_data.join("kodiak").join("ProjectsWiki");
        if is_valid(&direct_wiki) {
            return direct_wiki;
        }
    }
    // 5. Packaged install at /usr/share/kodiak/projectswiki or /usr/share/kodiak/ProjectsWiki (Linux)
    let sys_wiki = PathBuf::from("/usr/share/kodiak/projectswiki");
    if is_valid(&sys_wiki) {
        return sys_wiki;
    }
    let sys_wiki_cap = PathBuf::from("/usr/share/kodiak/ProjectsWiki");
    if is_valid(&sys_wiki_cap) {
        return sys_wiki_cap;
    }
    // 6. Local workspace relative path
    if let Ok(cwd) = std::env::current_dir() {
        let local_wiki = cwd.join("ProjectsWiki");
        if is_valid(&local_wiki) {
            return local_wiki;
        }
    }
    configured
}

/// Resolves the directory where the user's project repositories live.
/// Prioritizes configured `projects_dir`, then active project parent directory,
/// then first whitelist entry parent directory, then standard ~/Projects.
pub fn resolve_projects_dir(cfg: &AppConfig) -> PathBuf {
    let configured = expand_tilde(&cfg.projects_dir);
    if configured.exists() && configured.is_dir() {
        return configured;
    }
    // Check if active project has a valid parent directory
    if let Some(active) = &cfg.active_project {
        let active_path = expand_tilde(active);
        if let Some(parent) = active_path.parent() {
            if parent.exists() && parent.is_dir() && parent != Path::new("/") {
                return parent.to_path_buf();
            }
        }
    }
    // Check first whitelist entry parent directory
    if let Some(first_wl) = cfg.whitelist.first() {
        let wl_path = expand_tilde(&first_wl.path);
        if let Some(parent) = wl_path.parent() {
            if parent.exists() && parent.is_dir() && parent != Path::new("/") {
                return parent.to_path_buf();
            }
        }
    }
    // Fallback to standard ~/Projects location if present
    if let Some(home) = dirs::home_dir() {
        let std_projects = home.join("Projects");
        if std_projects.exists() {
            return std_projects;
        }
    }
    configured
}

pub fn config_file_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "Unable to resolve ~/.config directory".to_string())?;
    Ok(base.join("kodiak").join("config.toml"))
}

pub fn data_dir_path() -> Result<PathBuf, String> {
    if let Ok(override_dir) = std::env::var("KODIAK_DATA_DIR").or_else(|_| std::env::var("FOREMAN_DATA_DIR")) {
        let dir = PathBuf::from(override_dir);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data directory: {e}"))?;
        return Ok(dir);
    }
    let base = dirs::data_local_dir().ok_or_else(|| "Unable to resolve ~/.local/share directory".to_string())?;
    let dir = base.join("kodiak");
    if fs::create_dir_all(&dir).is_ok() {
        let test_file = dir.join(".write_test");
        if fs::write(&test_file, b"test").is_ok() {
            let _ = fs::remove_file(&test_file);
            return Ok(dir);
        }
    }
    let fallback = std::env::temp_dir().join("kodiak_data");
    let _ = fs::create_dir_all(&fallback);
    Ok(fallback)
}

pub fn journal_file_path() -> Result<PathBuf, String> {
    Ok(data_dir_path()?.join("journal.jsonl"))
}

pub fn history_file_path() -> Result<PathBuf, String> {
    Ok(data_dir_path()?.join("history.json"))
}

pub fn queue_file_path() -> Result<PathBuf, String> {
    Ok(data_dir_path()?.join("queue.json"))
}

pub fn cloud_debug_file_path() -> Result<PathBuf, String> {
    Ok(data_dir_path()?.join("cloud_debug.jsonl"))
}

pub fn memory_dir_path() -> Result<PathBuf, String> {
    let dir = data_dir_path()?.join("memory");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create memory directory: {e}"))?;
    Ok(dir)
}

pub fn runs_dir_path() -> Result<PathBuf, String> {
    let dir = data_dir_path()?.join("runs");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create runs directory: {e}"))?;
    Ok(dir)
}

pub fn task_run_dir_path(task_id: &str) -> Result<PathBuf, String> {
    let dir = runs_dir_path()?.join(task_id);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create task run directory: {e}"))?;
    Ok(dir)
}

pub fn load_or_seed_config() -> Result<AppConfig, String> {
    let cfg_path = config_file_path()?;
    if !cfg_path.exists() {
        let mut default_cfg = AppConfig::default();
        let profile = crate::hardware::detect_hardware_profile();
        default_cfg.fast_model = profile.recommended_models.fast_model;
        default_cfg.coder_model = profile.recommended_models.coder_model;
        default_cfg.heavy_model = profile.recommended_models.heavy_model;
        save_config(&default_cfg)?;
        return Ok(default_cfg);
    }
    let raw = fs::read_to_string(&cfg_path)
        .map_err(|e| format!("Failed to read {}: {e}", cfg_path.display()))?;
    let parsed: AppConfig = toml::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {e}", cfg_path.display()))?;
    if !raw.contains("local_idle_timeout_secs")
        || !raw.contains("local_total_timeout_secs")
        || !raw.contains("memory_step_budget_chars")
        || !raw.contains("memory_auto_sanitize")
        || !raw.contains("destructive_diff_threshold")
    {
        let _ = save_config(&parsed);
    }
    Ok(parsed)
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    let cfg_path = config_file_path()?;
    if let Some(parent) = cfg_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir {}: {e}", parent.display()))?;
    }
    let serialized = toml::to_string_pretty(cfg)
        .map_err(|e| format!("Failed to serialize config.toml: {e}"))?;
    fs::write(&cfg_path, serialized)
        .map_err(|e| format!("Failed to write {}: {e}", cfg_path.display()))?;
    Ok(())
}
