use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/morganlowe/Kodiak/releases/latest";
pub const ALLOWED_DOWNLOAD_PREFIX: &str =
    "https://github.com/morganlowe/Kodiak/releases/download/";
pub const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseAssetInfo {
    pub name: String,
    pub download_url: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub is_companion_wiki: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub update_available: bool,
    pub release_name: String,
    pub release_url: String,
    pub published_at: String,
    pub notes_summary: String,
    pub detected_platform: String,
    pub platform_kind: String,
    pub wiki_component_installed: bool,
    pub recommended_assets: Vec<ReleaseAssetInfo>,
    pub all_assets: Vec<ReleaseAssetInfo>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedUpdateAsset {
    pub name: String,
    pub saved_path: String,
    pub bytes: u64,
    pub computed_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    pub sha256_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDownloadResult {
    pub version: String,
    pub download_dir: String,
    pub files: Vec<DownloadedUpdateAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    pub can_launch_installer: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_installer_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubApiRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubApiAsset>,
}

fn parse_semver(raw: &str) -> ([u64; 3], Option<String>) {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match trimmed.split_once('-') {
        Some((c, p)) => {
            let p_clean = p.trim();
            (
                c,
                if p_clean.is_empty() {
                    None
                } else {
                    Some(p_clean.to_string())
                },
            )
        }
        None => (trimmed, None),
    };
    let mut nums = [0u64; 3];
    for (idx, part) in core.split('.').take(3).enumerate() {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = digits.parse::<u64>() {
            nums[idx] = v;
        }
    }
    (nums, pre)
}

/// Compares two semantic version strings (with optional leading `v` and pre-release suffix).
/// Stable releases order higher than pre-releases with the same `major.minor.patch`.
pub fn compare_semver(a: &str, b: &str) -> Ordering {
    let (a_nums, a_pre) = parse_semver(a);
    let (b_nums, b_pre) = parse_semver(b);
    match a_nums.cmp(&b_nums) {
        Ordering::Equal => match (&a_pre, &b_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(ap), Some(bp)) => ap.cmp(bp),
        },
        other => other,
    }
}

/// Detects the active OS packaging variant `(platform_kind, human_label, wiki_component_installed)`.
pub fn detect_platform_kind(wiki_enabled_fallback: bool) -> (String, String, bool) {
    #[cfg(target_os = "windows")]
    {
        let mut wiki_installed = wiki_enabled_fallback;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let bundled = exe_dir.join("resources").join("BenchBear");
                if bundled.join("kodiak-benchbear.exe").exists() || bundled.exists() {
                    wiki_installed = true;
                } else {
                    wiki_installed = false;
                }
            }
        }
        if wiki_installed {
            return (
                "windows-with-benchbear".to_string(),
                "Windows x64 (With BenchBear)".to_string(),
                true,
            );
        }
        return (
            "windows-without-benchbear".to_string(),
            "Windows x64 (Core Only)".to_string(),
            false,
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = wiki_enabled_fallback;
        let wiki_installed = Path::new("/usr/share/kodiak/benchbear").exists();

        if std::env::var_os("APPIMAGE").is_some() {
            return (
                "appimage".to_string(),
                "Linux AppImage".to_string(),
                wiki_installed,
            );
        }

        let os_release = fs::read_to_string("/etc/os-release")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_arch = os_release.contains("id=arch")
            || os_release.contains("id=cachyos")
            || os_release.contains("id=endeavouros")
            || os_release.contains("id=manjaro")
            || os_release.contains("id_like=arch")
            || os_release.contains("id_like=\"arch")
            || Path::new("/usr/bin/pacman").exists();
        if is_arch {
            return (
                "arch".to_string(),
                "Arch Linux / CachyOS (.pkg.tar.zst)".to_string(),
                wiki_installed,
            );
        }

        let is_deb = os_release.contains("id=debian")
            || os_release.contains("id=ubuntu")
            || os_release.contains("id_like=debian")
            || os_release.contains("id_like=ubuntu")
            || Path::new("/usr/bin/dpkg").exists();
        if is_deb {
            return (
                "debian".to_string(),
                "Debian / Ubuntu (.deb)".to_string(),
                wiki_installed,
            );
        }

        (
            "appimage".to_string(),
            "Linux Universal AppImage".to_string(),
            wiki_installed,
        )
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Extracts a 64-char SHA-256 hex digest for `asset_name` from either GitHub's `asset.digest` field
/// (`sha256:...`) or the release notes checksum table.
pub fn extract_sha256_for_asset(
    digest_field: Option<&str>,
    asset_name: &str,
    release_body: &str,
) -> Option<String> {
    if let Some(d) = digest_field {
        let trimmed = d.trim();
        let hex_part = trimmed.strip_prefix("sha256:").unwrap_or(trimmed).trim();
        if is_hex64(hex_part) {
            return Some(hex_part.to_ascii_lowercase());
        }
    }

    if asset_name.trim().is_empty() {
        return None;
    }

    for line in release_body.lines() {
        if !line.contains(asset_name) {
            continue;
        }
        for tok in line.split(|c: char| c.is_whitespace() || c == '|' || c == '`' || c == ',' || c == ';') {
            let clean = tok.trim().strip_prefix("sha256:").unwrap_or(tok.trim());
            if is_hex64(clean) {
                return Some(clean.to_ascii_lowercase());
            }
        }
    }

    None
}

/// Selects recommended release assets matching `platform_kind` and optional `kodiak-benchbear` companion.
pub fn select_recommended_assets(
    all_assets: &[ReleaseAssetInfo],
    platform_kind: &str,
    include_wiki: bool,
) -> Vec<ReleaseAssetInfo> {
    let mut selected = Vec::new();
    match platform_kind {
        "arch" => {
            if let Some(core) = all_assets.iter().find(|a| {
                a.name.starts_with("kodiak-")
                    && !a.name.contains("benchbear")
                    && a.name.ends_with(".pkg.tar.zst")
            }) {
                selected.push(core.clone());
            }
            if include_wiki {
                if let Some(wiki) = all_assets.iter().find(|a| {
                    a.name.starts_with("kodiak-benchbear-") && a.name.ends_with(".pkg.tar.zst")
                }) {
                    selected.push(wiki.clone());
                }
            }
        }
        "debian" => {
            if let Some(split_core) = all_assets.iter().find(|a| {
                a.name.starts_with("kodiak_")
                    && !a.name.contains("benchbear")
                    && a.name.ends_with(".deb")
            }) {
                selected.push(split_core.clone());
                if include_wiki {
                    if let Some(wiki) = all_assets.iter().find(|a| {
                        a.name.starts_with("kodiak-benchbear_") && a.name.ends_with(".deb")
                    }) {
                        selected.push(wiki.clone());
                    }
                }
            } else if let Some(bundle_deb) = all_assets
                .iter()
                .find(|a| a.name.ends_with(".deb") && !a.name.contains("benchbear"))
            {
                selected.push(bundle_deb.clone());
            }
        }
        "appimage" => {
            if let Some(appimg) = all_assets.iter().find(|a| a.name.ends_with(".AppImage")) {
                selected.push(appimg.clone());
            }
        }
        "windows-with-benchbear" => {
            if let Some(exe) = all_assets
                .iter()
                .find(|a| a.name.ends_with("-with-benchbear-setup.exe"))
            {
                selected.push(exe.clone());
            }
        }
        "windows-without-benchbear" => {
            if let Some(exe) = all_assets
                .iter()
                .find(|a| a.name.ends_with("-without-benchbear-setup.exe"))
            {
                selected.push(exe.clone());
            }
        }
        _ => {}
    }
    selected
}

/// Validates that an asset filename is safe (single path component) and URL matches GitHub Releases.
pub fn validate_asset_download(asset: &ReleaseAssetInfo) -> Result<(), String> {
    let name = asset.name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err(format!("Unsafe release asset filename: '{}'", asset.name));
    }
    if !asset.download_url.starts_with(ALLOWED_DOWNLOAD_PREFIX) {
        return Err(format!(
            "Blocked untrusted download URL '{}': must originate from {}",
            asset.download_url, ALLOWED_DOWNLOAD_PREFIX
        ));
    }
    Ok(())
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Builds the platform-appropriate install command or launcher metadata for downloaded files.
/// Never auto-executes `sudo`, `pacman`, `dpkg`, or `apt`.
pub fn build_install_command(
    platform_kind: &str,
    files: &[DownloadedUpdateAsset],
) -> (Option<String>, bool, Option<String>) {
    if files.is_empty() {
        return (None, false, None);
    }
    let quoted: Vec<String> = files.iter().map(|f| shell_single_quote(&f.saved_path)).collect();
    match platform_kind {
        "arch" => (
            Some(format!("sudo pacman -U {}", quoted.join(" "))),
            false,
            None,
        ),
        "debian" => (
            Some(format!(
                "sudo dpkg -i {} && sudo apt-get install -f",
                quoted.join(" ")
            )),
            false,
            None,
        ),
        "appimage" => {
            let first_path = files[0].saved_path.clone();
            let first_quoted = shell_single_quote(&first_path);
            (
                Some(format!("chmod +x {} && {}", first_quoted, first_quoted)),
                true,
                Some(first_path),
            )
        }
        "windows-with-benchbear" | "windows-without-benchbear" => {
            (None, true, Some(files[0].saved_path.clone()))
        }
        _ => (None, false, None),
    }
}

/// Produces a clean plain-text summary of release notes, omitting verbose checksum tables.
pub fn summarize_release_notes(body: &str, max_chars: usize) -> String {
    let mut kept = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Distribution Assets") || trimmed.starts_with("| Platform |") {
            break;
        }
        if trimmed == "---" && !kept.is_empty() {
            continue;
        }
        if !trimmed.is_empty() {
            kept.push(trimmed);
        }
    }
    let joined = kept.join("\n");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let prefix: String = joined.chars().take(max_chars).collect();
    format!("{prefix}\n...")
}

/// Queries the GitHub Releases API (`releases/latest`) on explicit user action or opt-in startup check.
pub async fn check_github_latest_release(
    wiki_enabled_fallback: bool,
) -> Result<UpdateCheckResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("Kodiak/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("Failed to initialize HTTP client for update check: {e}"))?;

    let resp = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "GitHub update check timed out after 15 seconds.".to_string()
            } else if e.is_connect() {
                format!("Could not reach api.github.com (network offline or blocked): {e}")
            } else {
                format!("Failed to query GitHub Releases: {e}")
            }
        })?;

    let status = resp.status();
    if status.as_u16() == 403 {
        return Err("GitHub API rate limit exceeded (HTTP 403). Try again later.".to_string());
    }
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "GitHub update check returned HTTP {}: {}",
            status.as_u16(),
            body_text.chars().take(200).collect::<String>()
        ));
    }

    let release: GithubApiRelease = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub release metadata: {e}"))?;

    let latest_version = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let update_available = compare_semver(&latest_version, CURRENT_APP_VERSION) == Ordering::Greater;
    let body_str = release.body.as_deref().unwrap_or("");
    let (platform_kind, detected_platform, wiki_component_installed) =
        detect_platform_kind(wiki_enabled_fallback);

    let all_assets: Vec<ReleaseAssetInfo> = release
        .assets
        .into_iter()
        .map(|a| {
            let sha256 = extract_sha256_for_asset(a.digest.as_deref(), &a.name, body_str);
            let is_companion_wiki = a.name.contains("benchbear");
            ReleaseAssetInfo {
                name: a.name,
                download_url: a.browser_download_url,
                size: a.size,
                sha256,
                is_companion_wiki,
            }
        })
        .collect();

    let recommended_assets =
        select_recommended_assets(&all_assets, &platform_kind, wiki_component_installed);

    Ok(UpdateCheckResult {
        current_version: CURRENT_APP_VERSION.to_string(),
        latest_version: latest_version.clone(),
        tag_name: release.tag_name,
        update_available,
        release_name: release
            .name
            .unwrap_or_else(|| format!("Kodiak v{latest_version}")),
        release_url: release.html_url,
        published_at: release.published_at.unwrap_or_default(),
        notes_summary: summarize_release_notes(body_str, 1600),
        detected_platform,
        platform_kind,
        wiki_component_installed,
        recommended_assets,
        all_assets,
        checked_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn resolve_updates_base_dir() -> Result<PathBuf, String> {
    let base = dirs::download_dir()
        .or_else(dirs::cache_dir)
        .ok_or_else(|| "Could not locate user Downloads or cache directory.".to_string())?;
    Ok(base.join("kodiak-updates"))
}

fn sanitize_version_slug(version: &str) -> Result<String, String> {
    let clean = version.trim().trim_start_matches(['v', 'V']);
    if clean.is_empty()
        || !clean
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(format!("Invalid version string '{}'", version));
    }
    Ok(clean.to_string())
}

/// Downloads selected GitHub release assets into `~/Downloads/kodiak-updates/v<ver>/` and verifies SHA-256.
pub async fn download_update_assets(
    version: &str,
    assets: &[ReleaseAssetInfo],
    platform_kind: &str,
) -> Result<UpdateDownloadResult, String> {
    if assets.is_empty() {
        return Err("No update package assets selected for download.".to_string());
    }
    let clean_ver = sanitize_version_slug(version)?;
    for asset in assets {
        validate_asset_download(asset)?;
    }

    let base_dir = resolve_updates_base_dir()?;
    let target_dir = base_dir.join(format!("v{clean_ver}"));
    fs::create_dir_all(&target_dir).map_err(|e| {
        format!(
            "Failed to create update directory '{}': {e}",
            target_dir.display()
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent(format!("Kodiak/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("Failed to create download client: {e}"))?;

    let mut downloaded = Vec::new();
    for asset in assets {
        let final_path = target_dir.join(&asset.name);
        let part_path = target_dir.join(format!("{}.part", asset.name));

        let mut resp = client
            .get(&asset.download_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download '{}': {e}", asset.name))?;
        if !resp.status().is_success() {
            return Err(format!(
                "Download failed for '{}' with HTTP {}",
                asset.name,
                resp.status().as_u16()
            ));
        }

        let mut file = File::create(&part_path)
            .map_err(|e| format!("Failed to create '{}': {e}", part_path.display()))?;
        let mut hasher = Sha256::new();
        let mut total_bytes: u64 = 0;

        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if let Err(e) = file.write_all(&chunk) {
                        let _ = fs::remove_file(&part_path);
                        return Err(format!("Write error for '{}': {e}", asset.name));
                    }
                    hasher.update(&chunk);
                    total_bytes += chunk.len() as u64;
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = fs::remove_file(&part_path);
                    return Err(format!("Stream interrupted downloading '{}': {e}", asset.name));
                }
            }
        }
        drop(file);

        let computed = format!("{:x}", hasher.finalize());
        let sha256_verified = if let Some(expected) = &asset.sha256 {
            let expected_norm = expected.trim().to_ascii_lowercase();
            if computed != expected_norm {
                let _ = fs::remove_file(&part_path);
                return Err(format!(
                    "SHA-256 verification FAILED for '{}': expected {}, got {}. Partial file removed.",
                    asset.name, expected_norm, computed
                ));
            }
            true
        } else {
            false
        };

        fs::rename(&part_path, &final_path).map_err(|e| {
            let _ = fs::remove_file(&part_path);
            format!(
                "Failed to finalize downloaded file '{}': {e}",
                final_path.display()
            )
        })?;

        #[cfg(unix)]
        if asset.name.ends_with(".AppImage") {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&final_path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&final_path, perms);
            }
        }

        downloaded.push(DownloadedUpdateAsset {
            name: asset.name.clone(),
            saved_path: final_path.to_string_lossy().to_string(),
            bytes: total_bytes,
            computed_sha256: computed,
            expected_sha256: asset.sha256.clone(),
            sha256_verified,
        });
    }

    let (install_command, can_launch_installer, primary_installer_path) =
        build_install_command(platform_kind, &downloaded);

    Ok(UpdateDownloadResult {
        version: clean_ver,
        download_dir: target_dir.to_string_lossy().to_string(),
        files: downloaded,
        install_command,
        can_launch_installer,
        primary_installer_path,
    })
}

/// Opens a previously verified `kodiak-updates` subdirectory in the OS file manager.
pub fn open_update_download_dir_checked(dir_path: &str) -> Result<(), String> {
    let base = resolve_updates_base_dir()?;
    fs::create_dir_all(&base).map_err(|e| format!("Failed to prepare updates dir: {e}"))?;
    let canon_base = base
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize updates dir: {e}"))?;
    let canon_target = Path::new(dir_path)
        .canonicalize()
        .map_err(|e| format!("Download directory '{}' not found: {e}", dir_path))?;

    if !canon_target.starts_with(&canon_base) || !canon_target.is_dir() {
        return Err(
            "Refusing to open directory outside Kodiak's update download folder.".to_string(),
        );
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&canon_target)
            .spawn()
            .map_err(|e| format!("Failed to open Explorer: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(&canon_target)
            .spawn()
            .map_err(|e| format!("Failed to open folder with xdg-open: {e}"))?;
    }
    Ok(())
}

/// Launches a downloaded standalone installer (`.exe` on Windows, `.AppImage` on Linux) inside `kodiak-updates`.
pub fn launch_downloaded_installer_checked(file_path: &str) -> Result<(), String> {
    let base = resolve_updates_base_dir()?;
    let canon_base = base
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize updates dir: {e}"))?;
    let canon_file = Path::new(file_path)
        .canonicalize()
        .map_err(|e| format!("Installer '{}' not found: {e}", file_path))?;

    if !canon_file.starts_with(&canon_base) || !canon_file.is_file() {
        return Err("Refusing to launch file outside Kodiak's update download folder.".to_string());
    }

    let name = canon_file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    #[cfg(target_os = "windows")]
    if !name.to_ascii_lowercase().ends_with(".exe") {
        return Err("Windows installer launcher requires a .exe file.".to_string());
    }
    #[cfg(not(target_os = "windows"))]
    if !name.ends_with(".AppImage") {
        return Err(
            "Direct installer launch on Linux only supports .AppImage; use the provided package command."
                .to_string(),
        );
    }

    Command::new(&canon_file)
        .spawn()
        .map_err(|e| format!("Failed to launch '{}': {e}", canon_file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn sample_assets() -> Vec<ReleaseAssetInfo> {
        vec![
            ReleaseAssetInfo {
                name: "kodiak-1.2.1-1-x86_64.pkg.tar.zst".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.2.1/kodiak-1.2.1-1-x86_64.pkg.tar.zst".to_string(),
                size: 4800000,
                sha256: Some("a".repeat(64)),
                is_companion_wiki: false,
            },
            ReleaseAssetInfo {
                name: "kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.3.1/kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst".to_string(),
                size: 134000,
                sha256: Some("b".repeat(64)),
                is_companion_wiki: true,
            },
            ReleaseAssetInfo {
                name: "kodiak_1.2.1-1_amd64.deb".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.2.1/kodiak_1.2.1-1_amd64.deb".to_string(),
                size: 4500000,
                sha256: Some("c".repeat(64)),
                is_companion_wiki: false,
            },
            ReleaseAssetInfo {
                name: "kodiak-benchbear_1.3.1-1_amd64.deb".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.3.1/kodiak-benchbear_1.3.1-1_amd64.deb".to_string(),
                size: 110000,
                sha256: Some("d".repeat(64)),
                is_companion_wiki: true,
            },
            ReleaseAssetInfo {
                name: "Kodiak_1.2.1_amd64.AppImage".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.2.1/Kodiak_1.2.1_amd64.AppImage".to_string(),
                size: 84000000,
                sha256: Some("e".repeat(64)),
                is_companion_wiki: false,
            },
            ReleaseAssetInfo {
                name: "Kodiak_1.3.1_x64-with-benchbear-setup.exe".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.3.1/Kodiak_1.3.1_x64-with-benchbear-setup.exe".to_string(),
                size: 40000000,
                sha256: Some("f".repeat(64)),
                is_companion_wiki: false,
            },
            ReleaseAssetInfo {
                name: "Kodiak_1.3.1_x64-without-benchbear-setup.exe".to_string(),
                download_url: "https://github.com/morganlowe/Kodiak/releases/download/v1.3.1/Kodiak_1.3.1_x64-without-benchbear-setup.exe".to_string(),
                size: 4200000,
                sha256: Some("0".repeat(64)),
                is_companion_wiki: false,
            },
        ]
    }

    #[test]
    fn test_compare_semver() {
        assert_eq!(compare_semver("v1.2.1", "1.2.0"), Ordering::Greater);
        assert_eq!(compare_semver("1.10.0", "1.2.9"), Ordering::Greater);
        assert_eq!(compare_semver("v1.2.0", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_semver("1.2.0-rc1", "1.2.0"), Ordering::Less);
        assert_eq!(compare_semver("1.1.9", "1.2.0"), Ordering::Less);
    }

    #[test]
    fn test_select_assets_for_arch_debian_appimage_and_windows() {
        let all = sample_assets();
        let arch_with = select_recommended_assets(&all, "arch", true);
        assert_eq!(arch_with.len(), 2);
        assert_eq!(arch_with[0].name, "kodiak-1.2.1-1-x86_64.pkg.tar.zst");
        assert_eq!(
            arch_with[1].name,
            "kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst"
        );

        let arch_core = select_recommended_assets(&all, "arch", false);
        assert_eq!(arch_core.len(), 1);
        assert_eq!(arch_core[0].name, "kodiak-1.2.1-1-x86_64.pkg.tar.zst");

        let deb_with = select_recommended_assets(&all, "debian", true);
        assert_eq!(deb_with.len(), 2);
        assert_eq!(deb_with[0].name, "kodiak_1.2.1-1_amd64.deb");

        let appimg = select_recommended_assets(&all, "appimage", false);
        assert_eq!(appimg.len(), 1);
        assert_eq!(appimg[0].name, "Kodiak_1.2.1_amd64.AppImage");

        let win_with = select_recommended_assets(&all, "windows-with-benchbear", true);
        assert_eq!(win_with[0].name, "Kodiak_1.3.1_x64-with-benchbear-setup.exe");

        let win_no = select_recommended_assets(&all, "windows-without-benchbear", false);
        assert_eq!(win_no[0].name, "Kodiak_1.3.1_x64-without-benchbear-setup.exe");
    }

    #[test]
    fn test_extract_sha256_from_digest_and_release_body() {
        let hex_a = "1d614ea776d66eac69dcc6568106b69d940b5774966cd3716b3a3a78cf036257";
        assert_eq!(
            extract_sha256_for_asset(Some(&format!("sha256:{hex_a}")), "kodiak.pkg", ""),
            Some(hex_a.to_string())
        );

        let body = format!(
            "| Arch | Core | `kodiak-1.2.0-1-x86_64.pkg.tar.zst` | 4.64 MiB | `{hex_a}` |"
        );
        assert_eq!(
            extract_sha256_for_asset(None, "kodiak-1.2.0-1-x86_64.pkg.tar.zst", &body),
            Some(hex_a.to_string())
        );
        assert_eq!(
            extract_sha256_for_asset(Some("invalid"), "other.pkg", &body),
            None
        );
    }

    #[test]
    fn test_validate_download_url_and_filename_safety() {
        let ok_asset = &sample_assets()[0];
        assert!(validate_asset_download(ok_asset).is_ok());

        let bad_url = ReleaseAssetInfo {
            download_url: "https://evil.example.com/kodiak.pkg.tar.zst".to_string(),
            ..ok_asset.clone()
        };
        assert!(validate_asset_download(&bad_url).is_err());

        for bad_name in ["../escape", "sub/file.deb", r"sub\file.exe", ""] {
            let bad_asset = ReleaseAssetInfo {
                name: bad_name.to_string(),
                ..ok_asset.clone()
            };
            assert!(validate_asset_download(&bad_asset).is_err());
        }
    }

    #[test]
    fn test_build_install_command_for_platforms() {
        let arch_files = vec![
            DownloadedUpdateAsset {
                name: "kodiak-1.2.1-1-x86_64.pkg.tar.zst".to_string(),
                saved_path: "/home/user/Downloads/kodiak-updates/v1.2.1/kodiak-1.2.1-1-x86_64.pkg.tar.zst".to_string(),
                bytes: 10,
                computed_sha256: "a".repeat(64),
                expected_sha256: None,
                sha256_verified: false,
            },
            DownloadedUpdateAsset {
                name: "kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst".to_string(),
                saved_path: "/home/user/Downloads/kodiak-updates/v1.3.1/kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst".to_string(),
                bytes: 10,
                computed_sha256: "b".repeat(64),
                expected_sha256: None,
                sha256_verified: false,
            },
        ];
        let (cmd, can_launch, primary) = build_install_command("arch", &arch_files);
        assert!(!can_launch);
        assert!(primary.is_none());
        let cmd_str = cmd.unwrap();
        assert!(cmd_str.starts_with("sudo pacman -U "));
        assert!(cmd_str.contains("kodiak-1.2.1-1-x86_64.pkg.tar.zst"));
        assert!(cmd_str.contains("kodiak-benchbear-1.3.1-1-x86_64.pkg.tar.zst"));

        let win_files = vec![DownloadedUpdateAsset {
            name: "Kodiak_1.3.1_x64-with-benchbear-setup.exe".to_string(),
            saved_path: r"C:\Users\u\Downloads\kodiak-updates\v1.3.1\Kodiak_1.3.1_x64-with-benchbear-setup.exe".to_string(),
            bytes: 10,
            computed_sha256: "f".repeat(64),
            expected_sha256: None,
            sha256_verified: false,
        }];
        let (w_cmd, w_launch, w_primary) = build_install_command("windows-with-benchbear", &win_files);
        assert!(w_cmd.is_none());
        assert!(w_launch);
        assert!(w_primary.unwrap().ends_with(".exe"));
    }

    #[test]
    fn test_config_check_for_updates_defaults_false() {
        let cfg = AppConfig::default();
        assert!(!cfg.check_for_updates);
        assert!(cfg.skipped_update_version.is_none());
    }
}
