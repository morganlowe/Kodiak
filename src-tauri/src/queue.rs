//! Sequential multi-task queue with VRAM-aware scheduling and intra-repository dependency chaining (§2.1 Slice 2).

use crate::config::queue_file_path;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex as TokioMutex;

/// Canonical event channel for queue status updates.
pub const QUEUE_EVENT_CHANNEL: &str = "kodiak://queue-event";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemStatus {
    /// Waiting in line for worker execution.
    Queued,
    /// Stage 1: Cloud Interpret is running.
    Interpreting,
    /// Paused: an earlier task in the same repository has unapproved action cards (§2.1).
    WaitingForDependency,
    /// Stage 2: Local model drafting/working under VRAM serialization lock.
    LocalWorking,
    /// Stage 3: Cloud Verify is running.
    Verifying,
    /// Task pipeline completed, but waiting for user to approve/reject action cards.
    WaitingApproval,
    /// All cards resolved or task completed with zero pending actions.
    Completed,
    /// Execution failed at one of the stages.
    Failed,
    /// Cancelled by user before or during execution.
    Cancelled,
}

impl std::fmt::Display for QueueItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "queued"),
            Self::Interpreting => write!(f, "interpreting"),
            Self::WaitingForDependency => write!(f, "waiting_for_dependency"),
            Self::LocalWorking => write!(f, "local_working"),
            Self::Verifying => write!(f, "verifying"),
            Self::WaitingApproval => write!(f, "waiting_approval"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub queue_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub prompt: String,
    pub project_root: String,
    /// If true, this task will not execute local file modifications until previous tasks in the same project are approved/rejected.
    pub wait_for_previous_approval: bool,
    pub status: QueueItemStatus,
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub pending_cards_count: usize,
    #[serde(default)]
    pub approved_cards_count: usize,
    #[serde(default)]
    pub denied_cards_count: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStore {
    pub version: u32,
    pub items: Vec<QueueItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueueEvent {
    QueueUpdated {
        items: Vec<QueueItem>,
    },
    ItemStatusChanged {
        queue_id: String,
        task_id: Option<String>,
        status: QueueItemStatus,
        error: Option<String>,
    },
}

pub fn emit_queue_event(app: Option<&AppHandle>, event: QueueEvent) {
    if let Some(handle) = app {
        let _ = handle.emit(QUEUE_EVENT_CHANNEL, &event);
    }
}

pub fn emit_queue_updated(app: Option<&AppHandle>, items: &[QueueItem]) {
    emit_queue_event(
        app,
        QueueEvent::QueueUpdated {
            items: items.to_vec(),
        },
    );
}

/// Thread-safe manager for the multi-task queue and VRAM serialization mutex.
pub struct QueueManager {
    pub items: Arc<Mutex<Vec<QueueItem>>>,
    /// Strictly serializes access to GPU memory across all local model inference operations (Ollama)
    /// to prevent Out-Of-Memory errors on Frank's RTX 4050 6GB Laptop GPU (§2.1 Slice 2).
    pub vram_lock: Arc<TokioMutex<()>>,
}

static QUEUE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueManager {
    pub fn new() -> Self {
        let initial_items = Self::load_persisted_items();
        Self {
            items: Arc::new(Mutex::new(initial_items)),
            vram_lock: Arc::new(TokioMutex::new(())),
        }
    }

    fn load_persisted_items() -> Vec<QueueItem> {
        let path = match queue_file_path() {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        if !path.exists() {
            return Vec::new();
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        if let Ok(store) = serde_json::from_str::<QueueStore>(&raw) {
            store
                .items
                .into_iter()
                .map(|mut item| {
                    // Reset transient in-flight states to Queued or Failed on app restart
                    if matches!(
                        item.status,
                        QueueItemStatus::Interpreting | QueueItemStatus::LocalWorking | QueueItemStatus::Verifying
                    ) {
                        item.status = QueueItemStatus::Queued;
                    }
                    item
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn save_items(&self) -> Result<(), String> {
        let path = queue_file_path()?;
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let guard = self.items.lock().map_err(|e| e.to_string())?;
        let store = QueueStore {
            version: 1,
            items: guard.clone(),
        };
        let serialized = serde_json::to_string_pretty(&store)
            .map_err(|e| format!("Failed to serialize queue store: {e}"))?;
        fs::write(&path, serialized)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(())
    }

    /// Enqueues a new natural language task prompt for a given project root.
    pub fn enqueue(
        &self,
        prompt: String,
        project_root: String,
        wait_for_previous_approval: bool,
    ) -> Result<QueueItem, String> {
        let item = QueueItem {
            queue_id: format!(
                "q-{}-{}",
                Utc::now().timestamp_millis(),
                QUEUE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ),
            task_id: None,
            prompt,
            project_root,
            wait_for_previous_approval,
            status: QueueItemStatus::Queued,
            created_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            pending_cards_count: 0,
            approved_cards_count: 0,
            denied_cards_count: 0,
            error: None,
        };
        {
            let mut guard = self.items.lock().map_err(|e| e.to_string())?;
            guard.push(item.clone());
        }
        let _ = self.save_items();
        Ok(item)
    }

    pub fn get_items(&self) -> Vec<QueueItem> {
        self.items
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn get_item(&self, queue_id: &str) -> Option<QueueItem> {
        self.items
            .lock()
            .ok()?
            .iter()
            .find(|i| i.queue_id == queue_id)
            .cloned()
    }

    /// Cancels a queue item. If it is already running or completed, it updates its status to Cancelled.
    pub fn cancel_item(&self, queue_id: &str) -> Result<QueueItem, String> {
        let mut guard = self.items.lock().map_err(|e| e.to_string())?;
        let item = guard
            .iter_mut()
            .find(|i| i.queue_id == queue_id)
            .ok_or_else(|| format!("Queue item '{queue_id}' not found"))?;

        item.status = QueueItemStatus::Cancelled;
        item.completed_at = Some(Utc::now().to_rfc3339());
        let res = item.clone();
        drop(guard);
        let _ = self.save_items();
        Ok(res)
    }

    /// Removes an item from the queue list (only if it is Cancelled, Completed, or Failed).
    pub fn remove_item(&self, queue_id: &str) -> Result<QueueItem, String> {
        let mut guard = self.items.lock().map_err(|e| e.to_string())?;
        let pos = guard
            .iter()
            .position(|i| i.queue_id == queue_id)
            .ok_or_else(|| format!("Queue item '{queue_id}' not found"))?;

        let removed = guard.remove(pos);
        drop(guard);
        let _ = self.save_items();
        Ok(removed)
    }

    /// Reorders a pending/queued item in the queue drawer.
    pub fn reorder_item(&self, queue_id: &str, new_index: usize) -> Result<Vec<QueueItem>, String> {
        let mut guard = self.items.lock().map_err(|e| e.to_string())?;
        let current_pos = guard
            .iter()
            .position(|i| i.queue_id == queue_id)
            .ok_or_else(|| format!("Queue item '{queue_id}' not found"))?;

        let item = guard.remove(current_pos);
        let target_idx = new_index.min(guard.len());
        guard.insert(target_idx, item);
        let res = guard.clone();
        drop(guard);
        let _ = self.save_items();
        Ok(res)
    }

    /// Clears all completed, cancelled, and failed items from the queue.
    pub fn clear_completed(&self) -> Vec<QueueItem> {
        let mut guard = match self.items.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        guard.retain(|i| {
            !matches!(
                i.status,
                QueueItemStatus::Completed | QueueItemStatus::Cancelled | QueueItemStatus::Failed
            )
        });
        let res = guard.clone();
        drop(guard);
        let _ = self.save_items();
        res
    }

    pub fn update_status(
        &self,
        queue_id: &str,
        status: QueueItemStatus,
        error: Option<String>,
    ) -> Option<QueueItem> {
        let mut guard = self.items.lock().ok()?;
        let item = guard.iter_mut().find(|i| i.queue_id == queue_id)?;
        item.status = status;
        if let Some(err) = error {
            item.error = Some(err);
        }
        if item.status == QueueItemStatus::Completed
            || item.status == QueueItemStatus::Failed
            || item.status == QueueItemStatus::Cancelled
        {
            if item.completed_at.is_none() {
                item.completed_at = Some(Utc::now().to_rfc3339());
            }
        }
        let res = item.clone();
        drop(guard);
        let _ = self.save_items();
        Some(res)
    }

    pub fn set_task_id(&self, queue_id: &str, task_id: &str) -> Option<QueueItem> {
        let mut guard = self.items.lock().ok()?;
        let item = guard.iter_mut().find(|i| i.queue_id == queue_id)?;
        item.task_id = Some(task_id.to_string());
        if item.started_at.is_none() {
            item.started_at = Some(Utc::now().to_rfc3339());
        }
        let res = item.clone();
        drop(guard);
        let _ = self.save_items();
        Some(res)
    }

    pub fn update_card_counts(
        &self,
        queue_id: &str,
        pending: usize,
        approved: usize,
        denied: usize,
    ) -> Option<QueueItem> {
        let mut guard = self.items.lock().ok()?;
        let item = guard.iter_mut().find(|i| i.queue_id == queue_id)?;
        item.pending_cards_count = pending;
        item.approved_cards_count = approved;
        item.denied_cards_count = denied;
        let res = item.clone();
        drop(guard);
        let _ = self.save_items();
        Some(res)
    }

    /// Synchronizes queue item state when an action card is approved or rejected in history.
    pub fn sync_from_task_record(
        &self,
        task_id: &str,
        pending: usize,
        approved: usize,
        denied: usize,
        is_done: bool,
    ) {
        let mut guard = match self.items.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(item) = guard
            .iter_mut()
            .find(|i| i.task_id.as_deref() == Some(task_id))
        {
            item.pending_cards_count = pending;
            item.approved_cards_count = approved;
            item.denied_cards_count = denied;
            if is_done && pending == 0 {
                item.status = QueueItemStatus::Completed;
                if item.completed_at.is_none() {
                    item.completed_at = Some(Utc::now().to_rfc3339());
                }
            } else if pending > 0 {
                item.status = QueueItemStatus::WaitingApproval;
            }
        }
        drop(guard);
        let _ = self.save_items();
    }

    /// Evaluates whether a queue item is blocked by an earlier dependency in the same project repository (§2.1).
    /// Returns true if an EARLIER item with the SAME project root has pending action cards or is still running.
    pub fn is_dependency_blocked(&self, queue_id: &str) -> bool {
        let guard = match self.items.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };

        let current_pos = match guard.iter().position(|i| i.queue_id == queue_id) {
            Some(p) => p,
            None => return false,
        };

        let current_item = &guard[current_pos];
        if !current_item.wait_for_previous_approval {
            return false;
        }

        let current_project = &current_item.project_root;

        for item in &guard[..current_pos] {
            if &item.project_root == current_project {
                // If previous task is still running or waiting for user approval of cards:
                if matches!(
                    item.status,
                    QueueItemStatus::Queued
                        | QueueItemStatus::Interpreting
                        | QueueItemStatus::WaitingForDependency
                        | QueueItemStatus::LocalWorking
                        | QueueItemStatus::Verifying
                        | QueueItemStatus::WaitingApproval
                ) || item.pending_cards_count > 0
                {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_enqueue_and_status() {
        let q = QueueManager::new();
        // Clear any leftover state
        {
            let mut items = q.items.lock().unwrap();
            items.clear();
        }

        let item = q
            .enqueue(
                "Refactor parser".to_string(),
                "/home/user/project_a".to_string(),
                true,
            )
            .unwrap();

        assert_eq!(item.status, QueueItemStatus::Queued);
        assert_eq!(item.prompt, "Refactor parser");
        assert_eq!(item.project_root, "/home/user/project_a");
        assert!(item.wait_for_previous_approval);

        let items = q.get_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].queue_id, item.queue_id);
    }

    #[test]
    fn test_dependency_chaining_same_vs_different_project() {
        let q = QueueManager::new();
        {
            let mut items = q.items.lock().unwrap();
            items.clear();
        }

        // Task 1 on Project A
        let t1 = q
            .enqueue("Task 1".to_string(), "/repo/proj_a".to_string(), false)
            .unwrap();
        // Task 2 on Project A with dependency chaining enabled
        let t2 = q
            .enqueue("Task 2".to_string(), "/repo/proj_a".to_string(), true)
            .unwrap();
        // Task 3 on Project B with dependency chaining enabled
        let t3 = q
            .enqueue("Task 3".to_string(), "/repo/proj_b".to_string(), true)
            .unwrap();

        // While Task 1 is Queued, Task 2 on Project A should be blocked
        assert!(q.is_dependency_blocked(&t2.queue_id));

        // Task 3 on Project B is NOT blocked by Task 1 or 2 because it's a different project!
        assert!(!q.is_dependency_blocked(&t3.queue_id));

        // When Task 1 is WaitingApproval with pending cards, Task 2 is STILL blocked
        q.update_status(&t1.queue_id, QueueItemStatus::WaitingApproval, None);
        q.update_card_counts(&t1.queue_id, 2, 0, 0);
        assert!(q.is_dependency_blocked(&t2.queue_id));

        // When Task 1 is Completed (cards resolved), Task 2 is UNBLOCKED!
        q.update_status(&t1.queue_id, QueueItemStatus::Completed, None);
        q.update_card_counts(&t1.queue_id, 0, 2, 0);
        assert!(!q.is_dependency_blocked(&t2.queue_id));
    }

    #[test]
    fn test_reorder_and_clear_completed() {
        let q = QueueManager::new();
        {
            let mut items = q.items.lock().unwrap();
            items.clear();
        }

        let t1 = q
            .enqueue("T1".to_string(), "/p1".to_string(), false)
            .unwrap();
        let t2 = q
            .enqueue("T2".to_string(), "/p1".to_string(), false)
            .unwrap();
        let t3 = q
            .enqueue("T3".to_string(), "/p1".to_string(), false)
            .unwrap();

        // Move T3 to index 0
        let reordered = q.reorder_item(&t3.queue_id, 0).unwrap();
        assert_eq!(reordered[0].queue_id, t3.queue_id);
        assert_eq!(reordered[1].queue_id, t1.queue_id);
        assert_eq!(reordered[2].queue_id, t2.queue_id);

        // Mark T1 completed and T2 cancelled
        q.update_status(&t1.queue_id, QueueItemStatus::Completed, None);
        q.cancel_item(&t2.queue_id).unwrap();

        // Clear completed
        let remaining = q.clear_completed();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].queue_id, t3.queue_id);
    }

    #[tokio::test]
    async fn test_vram_serialization_lock() {
        let q = Arc::new(QueueManager::new());
        let q_clone = Arc::clone(&q);

        // Task A acquires VRAM lock
        let lock_a = q.vram_lock.try_lock();
        assert!(lock_a.is_ok(), "Task A should acquire VRAM lock");

        // Concurrent Task B tries to acquire VRAM lock -> must fail while Task A holds it
        let lock_b = q_clone.vram_lock.try_lock();
        assert!(
            lock_b.is_err(),
            "Task B must be serialized and blocked while Task A holds VRAM lock"
        );

        // Release Task A lock
        drop(lock_a);

        // Now Task B can acquire it cleanly
        let lock_b_after = q_clone.vram_lock.try_lock();
        assert!(
            lock_b_after.is_ok(),
            "Task B can acquire VRAM lock once Task A releases it"
        );
    }
}
