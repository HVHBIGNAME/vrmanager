//! Installation queue owned by the Rust core.
//!
//! The queue deliberately lives here and not in the UI: closing a window must
//! never cancel work that is already touching a device, and the aggregate
//! result of a batch must be decided by whoever observed every item — not by a
//! React component counting events it may have missed.
//!
//! One job = one user action ("install these APKs on these devices"), expanded
//! into one item per (apk, device) pair. Every state change bumps a monotonic
//! `revision`, so a UI that reconnects can tell a stale snapshot from a fresh
//! one and never renders an older state over a newer one.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::core::cancel::CancelToken;
use crate::error::UiError;

pub use super::snapshot::{
    InstallItemSnapshot, InstallJobSnapshot, ItemStatus, JobStatus, QueuedInstall,
};

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

struct JobState {
    snapshot: InstallJobSnapshot,
    cancel: Option<CancelToken>,
}

/// Registry of installation jobs.
///
/// Keeps a bounded history so a UI that remounts (or opens the activity screen
/// later) can still see what happened, without growing without limit.
pub struct InstallQueue {
    registry: RwLock<Registry>,
    history_limit: usize,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, JobState>,
    order: Vec<String>,
}

/// How many terminal jobs to retain for the activity view.
const HISTORY_LIMIT: usize = 100;
pub const MAX_ITEMS: usize = 512;
pub const MAX_ACTIVE_JOBS: usize = 16;

impl InstallQueue {
    pub fn new() -> Self {
        Self {
            registry: RwLock::new(Registry::default()),
            history_limit: HISTORY_LIMIT,
        }
    }

    /// Create a job from (apk, device) pairs. Order is preserved: APKs in the
    /// order given, devices in the order given for each APK.
    pub async fn create(
        &self,
        items: Vec<QueuedInstall>,
    ) -> Result<(InstallJobSnapshot, CancelToken), UiError> {
        if items.is_empty() {
            return Err(UiError::new(
                "emptyInstallJob",
                "Choose an app and a device to install to.",
            ));
        }
        if items.len() > MAX_ITEMS {
            return Err(UiError::new(
                "selectionTooLarge",
                "Choose at most 512 app/device pairs.",
            ));
        }
        super::input::validate_items(&items)?;
        let job_id = uuid::Uuid::new_v4().to_string();
        let cancel = CancelToken::new();
        let now = now_ms();

        let snapshot = InstallJobSnapshot {
            job_id: job_id.clone(),
            revision: 1,
            status: JobStatus::Queued,
            created_at_ms: now,
            updated_at_ms: now,
            items: items
                .into_iter()
                .map(|q| InstallItemSnapshot {
                    item_id: q.item_id,
                    serial: q.serial,
                    apk_path: q.apk_path.to_string_lossy().into_owned(),
                    package: None,
                    application_name: None,
                    status: ItemStatus::Queued,
                    stage: crate::events::InstallStage::Preparing,
                    percent: None,
                    error: None,
                })
                .collect(),
        };

        let mut registry = self.registry.write().await;
        // Admission and insertion use the same registry lock.
        if registry
            .jobs
            .values()
            .filter(|s| !s.snapshot.is_terminal())
            .count()
            >= MAX_ACTIVE_JOBS
        {
            return Err(UiError::new(
                "queueFull",
                "Wait for an active installation to finish before adding another.",
            )
            .retryable(true));
        }
        registry.jobs.insert(
            job_id.clone(),
            JobState {
                snapshot: snapshot.clone(),
                cancel: Some(cancel.clone()),
            },
        );
        registry.order.push(job_id);
        Self::prune_locked(&mut registry, self.history_limit);
        Ok((snapshot, cancel))
    }

    /// Apply a mutation to one item and return the new snapshot.
    pub async fn update_item<F>(
        &self,
        job_id: &str,
        item_id: &str,
        f: F,
    ) -> Option<InstallJobSnapshot>
    where
        F: FnOnce(&mut InstallItemSnapshot),
    {
        let mut registry = self.registry.write().await;
        let state = registry.jobs.get_mut(job_id)?;
        let item = state
            .snapshot
            .items
            .iter_mut()
            .find(|i| i.item_id == item_id)?;
        if item.status.is_terminal() {
            return None;
        }
        f(item);

        state.snapshot.revision += 1;
        state.snapshot.updated_at_ms = now_ms();
        state.snapshot.status = InstallJobSnapshot::derive_status(&state.snapshot.items);
        if state.snapshot.is_terminal() {
            state.cancel.take();
        }
        let snapshot = state.snapshot.clone();
        Self::prune_locked(&mut registry, self.history_limit);
        Some(snapshot)
    }

    pub async fn get(&self, job_id: &str) -> Option<InstallJobSnapshot> {
        self.registry
            .read()
            .await
            .jobs
            .get(job_id)
            .map(|s| s.snapshot.clone())
    }

    /// Newest first, so the activity screen shows current work at the top.
    pub async fn list(&self) -> Vec<InstallJobSnapshot> {
        let registry = self.registry.read().await;
        registry
            .order
            .iter()
            .rev()
            .filter_map(|id| registry.jobs.get(id).map(|s| s.snapshot.clone()))
            .collect()
    }

    /// Cancel a job: stops the in-flight child and marks every item that has
    /// not started yet as cancelled, so nothing is left hanging in `queued`.
    pub async fn cancel(&self, job_id: &str) -> Option<InstallJobSnapshot> {
        let mut registry = self.registry.write().await;
        let state = registry.jobs.get_mut(job_id)?;
        if state.snapshot.is_terminal() {
            return Some(state.snapshot.clone());
        }
        if let Some(cancel) = state.cancel.take() {
            cancel.cancel();
        }
        for item in &mut state.snapshot.items {
            if !item.status.is_terminal() {
                item.status = ItemStatus::Cancelled;
                item.stage = crate::events::InstallStage::Failed;
            }
        }
        state.snapshot.revision += 1;
        state.snapshot.updated_at_ms = now_ms();
        state.snapshot.status = InstallJobSnapshot::derive_status(&state.snapshot.items);
        if state.snapshot.is_terminal() {
            state.cancel.take();
        }
        let snapshot = state.snapshot.clone();
        Self::prune_locked(&mut registry, self.history_limit);
        Some(snapshot)
    }

    /// Items worth retrying: failures the device could plausibly accept on a
    /// second attempt, plus anything that was cancelled before it ran.
    /// An ABI mismatch is not retryable and is deliberately excluded.
    pub async fn retryable_items(&self, job_id: &str) -> Vec<QueuedInstall> {
        let registry = self.registry.read().await;
        let Some(state) = registry.jobs.get(job_id) else {
            return Vec::new();
        };
        if !state.snapshot.is_terminal() {
            return Vec::new();
        }
        state
            .snapshot
            .items
            .iter()
            .filter(|i| match i.status {
                ItemStatus::Cancelled => true,
                ItemStatus::Failed => i.error.as_ref().map(|e| e.retryable).unwrap_or(false),
                _ => false,
            })
            .map(|i| QueuedInstall {
                item_id: uuid::Uuid::new_v4().to_string(),
                serial: i.serial.clone(),
                apk_path: PathBuf::from(&i.apk_path),
            })
            .collect()
    }

    pub async fn cancel_token(&self, job_id: &str) -> Option<CancelToken> {
        self.registry
            .read()
            .await
            .jobs
            .get(job_id)
            .and_then(|s| s.cancel.clone())
    }

    /// Drop the oldest terminal jobs once the history grows past the limit.
    /// Running jobs are never pruned.
    #[cfg(test)]
    async fn prune(&self) {
        let mut registry = self.registry.write().await;
        Self::prune_locked(&mut registry, self.history_limit);
    }

    fn prune_locked(registry: &mut Registry, history_limit: usize) {
        let terminal: Vec<String> = registry
            .order
            .iter()
            .filter(|id| {
                registry
                    .jobs
                    .get(*id)
                    .map(|s| s.snapshot.is_terminal())
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let excess = terminal.len().saturating_sub(history_limit);
        for id in terminal.into_iter().take(excess) {
            registry.jobs.remove(&id);
            registry.order.retain(|o| o != &id);
        }
    }
}

impl Default for InstallQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle used by the commands layer.
pub type SharedQueue = Arc<InstallQueue>;

#[cfg(test)]
#[path = "queue_regressions.rs"]
mod regressions;

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
