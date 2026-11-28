use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub const GITHUB_REPO: &str = "morganlowe/BenchBear";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub repo: String,
    pub current_version: String,
    pub local_commit: String,
    pub latest_version: String,
    pub latest_tag: String,
    pub update_available: bool,
    pub release_name: String,
    pub release_url: String,
    pub published_at: String,
    pub release_notes: String,
    pub checked_at: String,
    pub update_command: String,
    pub error: Option<String>,
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?.trim();
    if core.is_empty() {
        return None;
    }
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next().unwrap_or("0").parse().ok()?;
    let patch: u64 = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub fn detect_local_commit(wiki_root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(wiki_root)
        .args(["rev-parse", "--short", "HEAD"])
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn truncate_notes(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut end_byte = trimmed.len();
    for (idx, (bpos, _)) in trimmed.char_indices().enumerate() {
        if idx == max_chars {
            end_byte = bpos;
            break;
        }
    }
    format!("{}...", trimmed[..end_byte].trim_end())
}

fn build_base_result(wiki_root: &Path) -> UpdateCheckResult {
    UpdateCheckResult {
        repo: GITHUB_REPO.to_string(),
        current_version: CURRENT_VERSION.to_string(),
        local_commit: detect_local_commit(wiki_root),
        latest_version: String::new(),
        latest_tag: String::new(),
        update_available: false,
        release_name: String::new(),
        release_url: format!("https://github.com/{}/releases", GITHUB_REPO),
        published_at: String::new(),
        release_notes: String::new(),
        checked_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        update_command: "./wiki.sh update".to_string(),
        error: None,
    }
}

pub fn apply_release_json(res: &mut UpdateCheckResult, val: &Value) {
    let tag = val
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let latest_ver = tag.trim_start_matches(['v', 'V']).to_string();
    let name = val
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let url = val
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let published = val
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let body = val
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| truncate_notes(s, 1200))
        .unwrap_or_default();

    if !tag.is_empty() {
        res.latest_tag = tag;
    }
    if !latest_ver.is_empty() {
        res.latest_version = latest_ver.clone();
        res.update_available = is_newer_version(&latest_ver, &res.current_version);
    }
    res.release_name = name;
    if !url.is_empty() {
        res.release_url = url;
    }
    res.published_at = published;
    res.release_notes = body;
}

pub async fn check_github_updates(wiki_root: &Path) -> UpdateCheckResult {
    let mut res = build_base_result(wiki_root);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!("BenchBear/{}", CURRENT_VERSION))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            res.error = Some(format!("HTTP client initialization failed: {}", e));
            return res;
        }
    };

    let release_api = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    match fetch_github_json(&client, &release_api).await {
        Ok((200, val)) => {
            apply_release_json(&mut res, &val);
            return res;
        }
        Ok((404, _)) => {
            // Repository has no published GitHub Release yet; fall back to git tags.
            let tags_api = format!("https://api.github.com/repos/{}/tags?per_page=1", GITHUB_REPO);
            match fetch_github_json(&client, &tags_api).await {
                Ok((200, val)) => {
                    if let Some(tag) = val
                        .as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|item| item.get("name"))
                        .and_then(|v| v.as_str())
                    {
                        let tag = tag.trim().to_string();
                        let ver = tag.trim_start_matches(['v', 'V']).to_string();
                        res.latest_tag = tag.clone();
                        res.latest_version = ver.clone();
                        res.release_name = format!("Tag {}", tag);
                        res.update_available = is_newer_version(&ver, &res.current_version);
                    }
                    return res;
                }
                Ok((status, val)) => {
                    let msg = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("GitHub tags API error");
                    res.error = Some(format!("GitHub returned HTTP {}: {}", status, msg));
                    return res;
                }
                Err(e) => {
                    res.error = Some(format!("Could not reach GitHub tags API: {}", e));
                    return res;
                }
            }
        }
        Ok((status, val)) => {
            let msg = val
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("GitHub releases API error");
            res.error = Some(format!("GitHub returned HTTP {}: {}", status, msg));
            return res;
        }
        Err(e) => {
            res.error = Some(format!("Could not reach GitHub: {}", e));
            return res;
        }
    }
}

async fn fetch_github_json(client: &reqwest::Client, url: &str) -> Result<(u16, Value)> {
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    let status = resp.status().as_u16();
    let text = resp.text().await?;
    let val: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok((status, val))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semver_comparison_handles_v_prefix_and_ordering() {
        assert_eq!(parse_semver("v1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_semver("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_semver("v2.3.4-rc.1+build5"), Some((2, 3, 4)));

        assert!(is_newer_version("v1.0.1", "1.0.0"));
        assert!(is_newer_version("v1.10.0", "1.2.0"));
        assert!(!is_newer_version("v1.0.0", "1.0.0"));
        assert!(!is_newer_version("v0.9.9", "1.0.0"));
    }

    #[test]
    fn parses_github_release_json() {
        let mut res = build_base_result(Path::new("."));
        res.current_version = "1.0.0".to_string();
        let sample = json!({
            "tag_name": "v1.2.0",
            "name": "BenchBear v1.2.0",
            "html_url": "https://github.com/morganlowe/BenchBear/releases/tag/v1.2.0",
            "published_at": "2026-11-28T00:00:00Z",
            "body": "Added GitHub update checker and Final Cost calculator."
        });
        apply_release_json(&mut res, &sample);

        assert_eq!(res.latest_tag, "v1.2.0");
        assert_eq!(res.latest_version, "1.2.0");
        assert!(res.update_available);
        assert_eq!(res.release_name, "BenchBear v1.2.0");
        assert_eq!(
            res.release_url,
            "https://github.com/morganlowe/BenchBear/releases/tag/v1.2.0"
        );
        assert!(res.release_notes.contains("Final Cost calculator"));
    }
}
