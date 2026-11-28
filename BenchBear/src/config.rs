use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AppConfig {
    pub wiki_root: PathBuf,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub docs_dir: PathBuf,
    pub inventory_dir: PathBuf,
    pub projects_dir: PathBuf,
    pub site_domain: String,
    pub port_web: u16,
    pub port_api: u16,
    pub port_inventory: u16,
    pub default_currency: String,
    pub default_label_preset: String,
}

impl AppConfig {
    pub fn load(
        cli_wiki_root: Option<&Path>,
        cli_data_dir: Option<&Path>,
        cli_port: Option<u16>,
    ) -> Self {
        let wiki_root = resolve_wiki_root(cli_wiki_root);
        let mut site_domain = "benchbear.localhost".to_string();
        let mut port_web: u16 = 8000;
        let mut port_api: u16 = 8765;
        let mut port_inventory: u16 = 8090;
        let mut data_dir = wiki_root.join("data");
        let mut default_currency = "USD".to_string();
        let mut default_label_preset = "2x1".to_string();

        if let Some(cfg_path) = find_config_file(&wiki_root) {
            if let Ok(raw) = fs::read_to_string(&cfg_path) {
                if let Ok(val) = raw.parse::<toml::Value>() {
                    if let Some(v) = val.get("site_domain").and_then(|x| x.as_str()) {
                        if !v.is_empty() {
                            site_domain = v.to_string();
                        }
                    }
                    if let Some(v) = val.get("port_web").and_then(|x| x.as_integer()) {
                        if (1..=65535).contains(&v) {
                            port_web = v as u16;
                        }
                    }
                    if let Some(v) = val.get("port_api").and_then(|x| x.as_integer()) {
                        if (1..=65535).contains(&v) {
                            port_api = v as u16;
                        }
                    }
                    if let Some(v) = val.get("port_inventory").and_then(|x| x.as_integer()) {
                        if (1..=65535).contains(&v) {
                            port_inventory = v as u16;
                        }
                    }
                    if let Some(v) = val.get("data_dir").and_then(|x| x.as_str()) {
                        if !v.is_empty() {
                            data_dir = resolve_rel_or_home(&wiki_root, v);
                        }
                    }
                    if let Some(v) = val.get("default_currency").and_then(|x| x.as_str()) {
                        if !v.is_empty() {
                            default_currency = v.to_string();
                        }
                    }
                    if let Some(v) = val.get("default_label_preset").and_then(|x| x.as_str()) {
                        if !v.is_empty() {
                            default_label_preset = v.to_string();
                        }
                    }
                }
            }
        }

        if let Ok(v) = env::var("WIKI_DOMAIN") {
            if !v.is_empty() {
                site_domain = v;
            }
        }
        if let Ok(v) = env::var("WIKI_PORT_WEB") {
            if let Ok(p) = v.parse::<u16>() {
                port_web = p;
            }
        }
        if let Ok(v) = env::var("WIKI_PORT_API") {
            if let Ok(p) = v.parse::<u16>() {
                port_api = p;
            }
        }
        if let Ok(v) = env::var("WIKI_PORT_INVENTORY") {
            if let Ok(p) = v.parse::<u16>() {
                port_inventory = p;
            }
        }
        if let Ok(v) = env::var("WIKI_DATA_DIR") {
            if !v.is_empty() {
                data_dir = resolve_rel_or_home(&wiki_root, &v);
            }
        }
        if let Ok(v) = env::var("WIKI_CURRENCY") {
            if !v.is_empty() {
                default_currency = v;
            }
        }
        if let Ok(v) = env::var("WIKI_LABEL_PRESET") {
            if !v.is_empty() {
                default_label_preset = v;
            }
        }

        if let Some(d) = cli_data_dir {
            data_dir = if d.is_absolute() {
                d.to_path_buf()
            } else {
                wiki_root.join(d)
            };
        }
        if let Some(p) = cli_port {
            port_inventory = p;
        }

        let db_path = data_dir.join("inventory.db");
        let docs_dir = wiki_root.join("docs");
        let inventory_dir = wiki_root.join("inventory");
        let projects_dir = wiki_root.join("projects");

        Self {
            wiki_root,
            data_dir,
            db_path,
            docs_dir,
            inventory_dir,
            projects_dir,
            site_domain,
            port_web,
            port_api,
            port_inventory,
            default_currency,
            default_label_preset,
        }
    }
}

fn resolve_wiki_root(cli_wiki_root: Option<&Path>) -> PathBuf {
    if let Some(p) = cli_wiki_root {
        return fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    }
    if let Ok(v) = env::var("WIKI_ROOT") {
        if !v.is_empty() {
            let p = expand_home(&v);
            return fs::canonicalize(&p).unwrap_or(p);
        }
    }
    if let Ok(cwd) = env::current_dir() {
        if cwd.join("inventory").is_dir() || cwd.join("Cargo.toml").is_file() {
            return cwd;
        }
    }
    if let Ok(exe) = env::current_exe() {
        for ancestor in exe.ancestors() {
            if ancestor.join("Cargo.toml").is_file() || ancestor.join("inventory").is_dir() {
                return ancestor.to_path_buf();
            }
        }
    }
    if let Some(home) = home_dir() {
        let pkg_workspace = home.join(".local/share/benchbear/workspace");
        if pkg_workspace.join("inventory").is_dir() || pkg_workspace.join("data").is_dir() {
            return pkg_workspace;
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn find_config_file(wiki_root: &Path) -> Option<PathBuf> {
    if let Ok(p) = env::var("WIKI_CONFIG") {
        let path = expand_home(&p);
        if path.is_file() {
            return Some(path);
        }
    }
    let home = home_dir();
    let mut candidates = Vec::new();
    if let Some(ref h) = home {
        candidates.push(h.join(".config/benchbear/wiki.toml"));
    }
    candidates.push(wiki_root.join("wiki.toml"));
    candidates.into_iter().find(|p| p.is_file())
}

fn home_dir() -> Option<PathBuf> {
    env::var("HOME")
        .ok()
        .or_else(|| env::var("USERPROFILE").ok())
        .map(PathBuf::from)
}

fn expand_home(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(h) = home_dir() {
            return h.join(rest);
        }
    } else if raw == "~" {
        if let Some(h) = home_dir() {
            return h;
        }
    }
    PathBuf::from(raw)
}

fn resolve_rel_or_home(base: &Path, raw: &str) -> PathBuf {
    let expanded = expand_home(raw);
    if expanded.is_absolute() {
        expanded
    } else {
        base.join(expanded)
    }
}
