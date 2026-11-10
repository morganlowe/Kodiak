//! Multi-Project Health & Activity Dashboard (§2.3 Slice 2 Phase C).

use crate::config::AppConfig;
use crate::history::TaskRecord;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDashboardCard {
    pub path: String,
    pub name: String,
    pub grant_level: String,
    pub is_active: bool,
    pub is_git_repo: bool,
    pub git_branch: Option<String>,
    pub git_dirty_count: usize,
    pub total_tasks_count: usize,
    pub pending_approval_count: usize,
    pub last_task_id: Option<String>,
    pub last_task_prompt: Option<String>,
    pub last_task_verdict: Option<String>,
    pub last_task_timestamp: Option<String>,
    pub wiki_doc_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySummary {
    pub total_tasks: usize,
    pub total_local_actions: usize,
    pub total_cloud_calls: usize,
    pub zero_cloud_token_ratio_pct: f64,
    pub estimated_tokens_saved: usize,
    pub total_verified_tasks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiProjectDashboardResponse {
    pub projects: Vec<ProjectDashboardCard>,
    pub telemetry: TelemetrySummary,
}

/// Inspects git status for a target directory.
pub fn inspect_git_status(dir: &Path) -> (bool, Option<String>, usize) {
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output()
        .ok()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);

    if !is_git {
        return (false, None, 0);
    }

    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            } else {
                None
            }
        });

    let dirty_count = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .ok()
        .map(|o| {
            if o.status.success() {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .count()
            } else {
                0
            }
        })
        .unwrap_or(0);

    (true, branch, dirty_count)
}

/// Gathers dashboard cards and local telemetry across all whitelisted projects (§2.3).
pub fn gather_dashboard_data(
    cfg: &AppConfig,
    history: &[TaskRecord],
) -> MultiProjectDashboardResponse {
    let active_project = cfg.active_project.as_deref().unwrap_or("");

    let mut projects = Vec::new();

    for entry in &cfg.whitelist {
        let p_path = Path::new(&entry.path);
        let name = p_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&entry.path)
            .to_string();

        let is_active = entry.path == active_project;
        let (is_git_repo, git_branch, git_dirty_count) = inspect_git_status(p_path);

        // Correlate tasks from history
        let project_tasks: Vec<&TaskRecord> = history
            .iter()
            .filter(|t| t.project_dir == entry.path)
            .collect();

        let total_tasks_count = project_tasks.len();
        let pending_approval_count = project_tasks
            .iter()
            .map(|t| {
                t.action_cards
                    .iter()
                    .filter(|c| c.status == "pending_approval")
                    .count()
            })
            .sum();

        let last_task = project_tasks.first(); // history is latest-first
        let (last_task_id, last_task_prompt, last_task_verdict, last_task_timestamp) = match last_task {
            Some(t) => (
                Some(t.task_id.clone()),
                Some(t.prompt.clone()),
                t.verification.as_ref().map(|v| v.verdict.clone()),
                Some(t.timestamp.clone()),
            ),
            None => (None, None, None, None),
        };

        let wiki_doc_exists = p_path.join("README.md").exists()
            || Path::new("ProjectsWiki/docs/projects")
                .join(format!("{name}.md"))
                .exists();

        projects.push(ProjectDashboardCard {
            path: entry.path.clone(),
            name,
            grant_level: format!("{:?}", entry.level).to_ascii_uppercase(),
            is_active,
            is_git_repo,
            git_branch,
            git_dirty_count,
            total_tasks_count,
            pending_approval_count,
            last_task_id,
            last_task_prompt,
            last_task_verdict,
            last_task_timestamp,
            wiki_doc_exists,
        });
    }

    let total_tasks = history.len();
    let total_local_actions: usize = history
        .iter()
        .map(|t| {
            t.auto_executed_count
                + t.action_cards
                    .iter()
                    .filter(|c| c.status == "approved")
                    .count()
        })
        .sum();

    let total_cloud_calls: usize = history
        .iter()
        .map(|t| {
            let mut calls = 0;
            if t.plan.is_some() {
                calls += 1;
            }
            if t.verification.is_some() {
                calls += 1;
            }
            calls
        })
        .sum();

    let total_ops = total_local_actions + total_cloud_calls;
    let zero_cloud_token_ratio_pct = if total_ops > 0 {
        ((total_local_actions as f64 / total_ops as f64) * 100.0 * 10.0).round() / 10.0
    } else {
        100.0
    };

    let estimated_tokens_saved = total_local_actions * 2000;

    let total_verified_tasks = history
        .iter()
        .filter(|t| {
            t.verification
                .as_ref()
                .map_or(false, |v| v.verdict.to_lowercase() == "pass")
        })
        .count();

    MultiProjectDashboardResponse {
        projects,
        telemetry: TelemetrySummary {
            total_tasks,
            total_local_actions,
            total_cloud_calls,
            zero_cloud_token_ratio_pct,
            estimated_tokens_saved,
            total_verified_tasks,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GrantLevel, WhitelistEntry};

    #[test]
    fn test_inspect_git_status_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        let (is_git, branch, _) = inspect_git_status(&cwd);
        assert!(is_git);
        assert!(branch.is_some());
    }

    #[test]
    fn test_gather_dashboard_data_empty_history() {
        let mut cfg = AppConfig::default();
        cfg.whitelist.push(WhitelistEntry {
            path: "/tmp/sample-project".to_string(),
            level: GrantLevel::Autonomous,
        });
        cfg.active_project = Some("/tmp/sample-project".to_string());

        let res = gather_dashboard_data(&cfg, &[]);
        assert_eq!(res.projects.len(), 1);
        assert_eq!(res.projects[0].name, "sample-project");
        assert!(res.projects[0].is_active);
        assert_eq!(res.projects[0].grant_level, "AUTONOMOUS");
        assert_eq!(res.telemetry.total_tasks, 0);
        assert_eq!(res.telemetry.zero_cloud_token_ratio_pct, 100.0);
    }
}
