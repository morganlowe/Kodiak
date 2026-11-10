//! Unified Queue Inbox, Diff Conflict Detection, and Post-Approval Test Hooks (§2.2 Slice 2).

use crate::history::ActionCard;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchCardTarget {
    pub task_id: String,
    pub card_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestHookResult {
    pub project_dir: String,
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub raw_output: String,
    pub digested_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchApprovalResult {
    pub approved_count: usize,
    pub rejected_count: usize,
    pub affected_projects: Vec<String>,
    pub test_hook_results: Vec<TestHookResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedInboxItem {
    pub task_id: String,
    pub task_title: String,
    pub project_dir: String,
    pub card: ActionCard,
    pub line_ranges: Vec<(usize, usize)>,
    pub has_conflict: bool,
    pub conflicting_tasks: Vec<String>,
    pub conflict_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedInboxResponse {
    pub items: Vec<UnifiedInboxItem>,
    pub total_pending: usize,
    pub conflict_count: usize,
}

/// Parses unified diff text to extract targeted hunk line ranges `(start_line, end_line)`.
pub fn extract_diff_line_ranges(diff_text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for line in diff_text.lines() {
        if line.starts_with("@@ ") {
            if let Some(end_idx) = line[3..].find("@@") {
                let header = line[3..3 + end_idx].trim();
                for part in header.split_whitespace() {
                    if let Some(stripped) = part.strip_prefix('+') {
                        let mut nums = stripped.split(',');
                        if let Some(start_str) = nums.next() {
                            if let Ok(start) = start_str.parse::<usize>() {
                                let len = nums
                                    .next()
                                    .and_then(|l| l.parse::<usize>().ok())
                                    .unwrap_or(1);
                                ranges.push((start, start + len.saturating_sub(1)));
                            }
                        }
                    }
                }
            }
        }
    }
    ranges
}

/// Determines whether two inclusive line ranges `[start, end]` overlap.
pub fn ranges_overlap(r1: (usize, usize), r2: (usize, usize)) -> bool {
    r1.0 <= r2.1 && r2.0 <= r1.1
}

/// Inspects proposed diffs across queued tasks and flags overlapping line ranges (§2.2).
pub fn detect_conflicts(items: &mut [UnifiedInboxItem]) {
    let len = items.len();
    for i in 0..len {
        for j in (i + 1)..len {
            // Only compare pending cards targeting the same project and same file path
            if items[i].project_dir == items[j].project_dir
                && items[i].card.target_path == items[j].card.target_path
                && !items[i].card.target_path.is_empty()
                && items[i].task_id != items[j].task_id
            {
                let ranges_i = &items[i].line_ranges;
                let ranges_j = &items[j].line_ranges;

                let mut overlaps = false;
                let mut overlap_detail = String::new();

                // If either has no explicit hunk ranges (e.g. whole-file rewrite/creation), they inherently conflict
                if ranges_i.is_empty() || ranges_j.is_empty() {
                    overlaps = true;
                    overlap_detail = format!(
                        "Whole-file modification collision on '{}'",
                        items[i].card.target_path
                    );
                } else {
                    for r_a in ranges_i {
                        for r_b in ranges_j {
                            if ranges_overlap(*r_a, *r_b) {
                                overlaps = true;
                                overlap_detail = format!(
                                    "Overlapping diff lines [{}-{}] vs [{}-{}] on '{}'",
                                    r_a.0, r_a.1, r_b.0, r_b.1, items[i].card.target_path
                                );
                                break;
                            }
                        }
                        if overlaps {
                            break;
                        }
                    }
                }

                if overlaps {
                    items[i].has_conflict = true;
                    if !items[i].conflicting_tasks.contains(&items[j].task_id) {
                        items[i].conflicting_tasks.push(items[j].task_id.clone());
                    }
                    items[i].conflict_description = Some(overlap_detail.clone());

                    items[j].has_conflict = true;
                    if !items[j].conflicting_tasks.contains(&items[i].task_id) {
                        items[j].conflicting_tasks.push(items[i].task_id.clone());
                    }
                    items[j].conflict_description = Some(overlap_detail);
                }
            }
        }
    }
}

/// Detects the standard verification/test command for a given project root (§2.2).
pub fn detect_project_test_command(project_path: &Path) -> Option<String> {
    if project_path.join("Cargo.toml").exists() {
        return Some("cargo check".to_string());
    }
    if project_path.join("package.json").exists() {
        return Some("npm test".to_string());
    }
    if project_path.join("pytest.ini").exists()
        || project_path.join("pyproject.toml").exists()
        || project_path.join("setup.py").exists()
    {
        return Some("pytest".to_string());
    }
    if project_path.join("Makefile").exists() {
        return Some("make test".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enforcer::ActionClass;

    #[test]
    fn test_extract_diff_line_ranges() {
        let diff = r#"--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,5 +10,8 @@
 fn first() {}
@@ -50,10 +53,12 @@
 fn second() {}
"#;
        let ranges = extract_diff_line_ranges(diff);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (10, 17)); // 10 + 8 - 1
        assert_eq!(ranges[1], (53, 64)); // 53 + 12 - 1
    }

    #[test]
    fn test_ranges_overlap() {
        assert!(ranges_overlap((10, 20), (15, 25)));
        assert!(ranges_overlap((15, 25), (10, 20)));
        assert!(ranges_overlap((10, 20), (20, 30))); // boundaries touch
        assert!(!ranges_overlap((10, 20), (21, 30))); // disjoint
        assert!(!ranges_overlap((30, 40), (10, 20)));
    }

    #[test]
    fn test_conflict_detection_same_file_overlapping_lines() {
        let mut card1 = ActionCard::default();
        card1.card_id = "c1".to_string();
        card1.target_path = "src/main.rs".to_string();
        card1.action_class = ActionClass::Overwrite;

        let mut card2 = ActionCard::default();
        card2.card_id = "c2".to_string();
        card2.target_path = "src/main.rs".to_string();
        card2.action_class = ActionClass::Overwrite;

        let mut items = vec![
            UnifiedInboxItem {
                task_id: "t1".to_string(),
                task_title: "Task 1".to_string(),
                project_dir: "/home/user/project_a".to_string(),
                card: card1,
                line_ranges: vec![(10, 25)],
                has_conflict: false,
                conflicting_tasks: Vec::new(),
                conflict_description: None,
            },
            UnifiedInboxItem {
                task_id: "t2".to_string(),
                task_title: "Task 2".to_string(),
                project_dir: "/home/user/project_a".to_string(),
                card: card2,
                line_ranges: vec![(20, 35)], // overlaps 20-25!
                has_conflict: false,
                conflicting_tasks: Vec::new(),
                conflict_description: None,
            },
        ];

        detect_conflicts(&mut items);

        assert!(items[0].has_conflict);
        assert_eq!(items[0].conflicting_tasks, vec!["t2"]);
        assert!(items[0].conflict_description.as_ref().unwrap().contains("Overlapping diff lines"));

        assert!(items[1].has_conflict);
        assert_eq!(items[1].conflicting_tasks, vec!["t1"]);
    }

    #[test]
    fn test_no_conflict_different_files_or_disjoint_lines() {
        let mut card1 = ActionCard::default();
        card1.target_path = "src/main.rs".to_string();

        let mut card2 = ActionCard::default();
        card2.target_path = "src/main.rs".to_string();

        let mut items = vec![
            UnifiedInboxItem {
                task_id: "t1".to_string(),
                task_title: "Task 1".to_string(),
                project_dir: "/home/user/project_a".to_string(),
                card: card1,
                line_ranges: vec![(10, 20)],
                has_conflict: false,
                conflicting_tasks: Vec::new(),
                conflict_description: None,
            },
            UnifiedInboxItem {
                task_id: "t2".to_string(),
                task_title: "Task 2".to_string(),
                project_dir: "/home/user/project_a".to_string(),
                card: card2,
                line_ranges: vec![(50, 70)], // disjoint!
                has_conflict: false,
                conflicting_tasks: Vec::new(),
                conflict_description: None,
            },
        ];

        detect_conflicts(&mut items);

        assert!(!items[0].has_conflict);
        assert!(!items[1].has_conflict);
    }

    #[test]
    fn test_detect_project_test_command() {
        let temp_dir = std::env::temp_dir().join("kodiak_test_cmd_detect");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(temp_dir.join("Cargo.toml"), "[package]\nname=\"foo\"").unwrap();
        assert_eq!(
            detect_project_test_command(&temp_dir),
            Some("cargo check".to_string())
        );

        let _ = std::fs::remove_file(temp_dir.join("Cargo.toml"));
        std::fs::write(temp_dir.join("package.json"), "{}").unwrap();
        assert_eq!(
            detect_project_test_command(&temp_dir),
            Some("npm test".to_string())
        );
    }
}
