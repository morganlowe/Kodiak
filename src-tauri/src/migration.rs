//! First-launch migration from Foreman to Kodiak (§2 Slice 2.6).
//!
//! Guarantees:
//! 1. Atomic moves on the same filesystem (no parsing, no transformation, zero data loss).
//! 2. If ~/.local/share/kodiak does not exist and ~/.local/share/foreman does, moves entire tree.
//! 3. Same for ~/.config/foreman -> ~/.config/kodiak.
//! 4. If both exist, logs warning and leaves both intact (never merge, never overwrite).
//! 5. If any move fails, aborts cleanly with a descriptive error naming the exact path and reason,
//!    leaving no half-migrated state.
//! 6. Journals a `migration_from_foreman` entry recording every moved path.

use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Performs migration with explicit base directories for configuration and data.
/// This allows deterministic unit testing using temporary directories.
pub fn migrate_paths_with_bases(
    config_base: &Path,
    data_base: &Path,
) -> Result<Vec<String>, String> {
    let old_data = data_base.join("foreman");
    let new_data = data_base.join("kodiak");
    let old_config = config_base.join("foreman");
    let new_config = config_base.join("kodiak");

    let mut moved: Vec<String> = Vec::new();
    let mut data_was_moved = false;

    // 1. Migrate Data Directory (~/.local/share/foreman -> ~/.local/share/kodiak)
    if new_data.exists() {
        if old_data.exists() {
            eprintln!(
                "Warning: Both '{}' and '{}' exist. Skipping data migration (never merge, never overwrite).",
                old_data.display(),
                new_data.display()
            );
        }
    } else if old_data.exists() {
        if let Some(parent) = new_data.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dir '{}': {e}", parent.display()))?;
        }
        fs::rename(&old_data, &new_data).map_err(|e| {
            format!(
                "Data migration failed: cannot rename '{}' to '{}': {e}",
                old_data.display(),
                new_data.display()
            )
        })?;
        data_was_moved = true;
        moved.push(format!("{} -> {}", old_data.display(), new_data.display()));
    }

    // 2. Migrate Config Directory (~/.config/foreman -> ~/.config/kodiak)
    if new_config.exists() {
        if old_config.exists() {
            eprintln!(
                "Warning: Both '{}' and '{}' exist. Skipping config migration (never merge, never overwrite).",
                old_config.display(),
                new_config.display()
            );
        }
    } else if old_config.exists() {
        if let Some(parent) = new_config.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                // Roll back data directory move if config parent creation fails
                if data_was_moved {
                    let _ = fs::rename(&new_data, &old_data);
                }
                return Err(format!(
                    "Config migration failed: cannot create parent dir '{}': {e}",
                    parent.display()
                ));
            }
        }
        if let Err(e) = fs::rename(&old_config, &new_config) {
            // Roll back data directory move to prevent partial migration state
            if data_was_moved {
                let _ = fs::rename(&new_data, &old_data);
            }
            return Err(format!(
                "Config migration failed: cannot rename '{}' to '{}': {e}",
                old_config.display(),
                new_config.display()
            ));
        }
        moved.push(format!("{} -> {}", old_config.display(), new_config.display()));
    }

    // 3. Journal migration_from_foreman entry in migrated journal.jsonl
    if !moved.is_empty() {
        let journal_path = new_data.join("journal.jsonl");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&journal_path) {
            let entry = serde_json::json!({
                "timestamp": Utc::now().to_rfc3339(),
                "task_id": "migration",
                "step_id": 0,
                "action_class": "execute",
                "target_path": "kodiak",
                "command_or_summary": "migration_from_foreman",
                "directory": "system",
                "grant_level": "AUTONOMOUS",
                "cruise_control": false,
                "dry_run": false,
                "decision": "migration_from_foreman",
                "reason": format!("Migrated paths: {}", moved.join("; "))
            });
            let _ = writeln!(file, "{}", entry);
        }
    }

    Ok(moved)
}

/// Runs first-launch migration using the user's standard ~/.config and ~/.local/share paths.
pub fn run_migration_if_needed() -> Result<Vec<String>, String> {
    let cfg_base = dirs::config_dir().ok_or_else(|| "Unable to resolve ~/.config directory".to_string())?;
    let data_base = dirs::data_local_dir().ok_or_else(|| "Unable to resolve ~/.local/share directory".to_string())?;
    migrate_paths_with_bases(&cfg_base, &data_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn test_migration_moves_dirs_atomically() {
        let root = std::env::temp_dir().join(format!("kodiak-mig-test-{}", Uuid::new_v4()));
        let cfg_base = root.join("config");
        let data_base = root.join("share");
        let old_cfg = cfg_base.join("foreman");
        let old_data = data_base.join("foreman");

        fs::create_dir_all(&old_cfg).unwrap();
        fs::create_dir_all(old_data.join("memory")).unwrap();
        fs::write(old_cfg.join("config.toml"), "command_timeout_secs = 60\n").unwrap();
        fs::write(old_data.join("history.json"), "[]\n").unwrap();
        fs::write(old_data.join("journal.jsonl"), "{\"initial\":true}\n").unwrap();
        fs::write(old_data.join("memory").join("m1.md"), "# Test Memory\n").unwrap();

        let moved = migrate_paths_with_bases(&cfg_base, &data_base).expect("Migration should succeed");
        assert_eq!(moved.len(), 2);

        // Verify old directories no longer exist
        assert!(!old_cfg.exists(), "Old config directory must be moved, not copied");
        assert!(!old_data.exists(), "Old data directory must be moved, not copied");

        // Verify new directories exist with 100% contents intact
        let new_cfg = cfg_base.join("kodiak");
        let new_data = data_base.join("kodiak");
        assert!(new_cfg.join("config.toml").exists());
        assert!(new_data.join("history.json").exists());
        assert!(new_data.join("journal.jsonl").exists());
        assert!(new_data.join("memory").join("m1.md").exists());

        // Verify journal entry recorded migration_from_foreman
        let journal_content = fs::read_to_string(new_data.join("journal.jsonl")).unwrap();
        assert!(journal_content.contains("migration_from_foreman"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_migration_never_merges_existing_dirs() {
        let root = std::env::temp_dir().join(format!("kodiak-mig-nomerge-{}", Uuid::new_v4()));
        let cfg_base = root.join("config");
        let data_base = root.join("share");

        let old_data = data_base.join("foreman");
        let new_data = data_base.join("kodiak");
        let old_cfg = cfg_base.join("foreman");
        let new_cfg = cfg_base.join("kodiak");

        fs::create_dir_all(&old_data).unwrap();
        fs::create_dir_all(&new_data).unwrap();
        fs::create_dir_all(&old_cfg).unwrap();
        fs::create_dir_all(&new_cfg).unwrap();

        fs::write(old_data.join("old.txt"), "old data").unwrap();
        fs::write(new_data.join("new.txt"), "new data").unwrap();
        fs::write(old_cfg.join("old_cfg.toml"), "old").unwrap();
        fs::write(new_cfg.join("config.toml"), "new").unwrap();

        let moved = migrate_paths_with_bases(&cfg_base, &data_base).expect("Should succeed with no-op");
        assert!(moved.is_empty(), "Should not move anything when both exist");

        // Verify old and new exist untouched, zero merging
        assert!(old_data.join("old.txt").exists());
        assert!(new_data.join("new.txt").exists());
        assert!(!new_data.join("old.txt").exists(), "Must never merge old into new");

        assert!(old_cfg.join("old_cfg.toml").exists());
        assert!(new_cfg.join("config.toml").exists());
        assert!(!new_cfg.join("old_cfg.toml").exists(), "Must never merge old config into new");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_migration_aborts_cleanly_on_failure() {
        let root = std::env::temp_dir().join(format!("kodiak-mig-fail-{}", Uuid::new_v4()));
        let cfg_base = root.join("config");
        let data_base = root.join("share");

        let old_data = data_base.join("foreman");
        let old_cfg = cfg_base.join("foreman");

        fs::create_dir_all(&old_data).unwrap();
        fs::create_dir_all(&old_cfg).unwrap();
        fs::write(old_data.join("important.json"), "critical data").unwrap();
        fs::write(old_cfg.join("config.toml"), "critical config").unwrap();

        // Inject permission failure: make cfg_base read-only so rename into it fails
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cfg_base, fs::Permissions::from_mode(0o555)).unwrap();
        }

        let res = migrate_paths_with_bases(&cfg_base, &data_base);

        // Restore permissions so we can inspect and clean up
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cfg_base, fs::Permissions::from_mode(0o755));
        }

        assert!(res.is_err(), "Migration must return an error when a rename fails");
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("Config migration failed"),
            "Error message must name the failed operation: {err_msg}"
        );

        // Verify rollback: old_data MUST still exist with its data intact (no half-migrated state!)
        assert!(old_data.exists(), "old_data must be rolled back and still exist");
        assert!(
            old_data.join("important.json").exists(),
            "Data in old directory must be intact"
        );
        assert!(
            !data_base.join("kodiak").exists(),
            "new_data must not be left behind in half-migrated state"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
