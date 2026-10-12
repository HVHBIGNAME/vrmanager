//! Shared application state handed to every Tauri command.

pub mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::core::adb::binary::{self, AdbBinary};
use crate::core::adb::client::AdbClient;
use crate::core::cancel::CancelToken;
use crate::core::devices::manager::DeviceManager;
use crate::error::{CoreError, CoreResult};

pub use settings::Settings;

/// Everything the commands layer needs. Cheap to clone (all `Arc` inside).
pub struct AppState {
    inner: RwLock<Option<Arc<DeviceManager>>>,
    settings: RwLock<Settings>,
    settings_path: PathBuf,
    profile_path: PathBuf,
    adb: RwLock<Option<AdbBinary>>,
    /// Cancellation tokens for in-flight jobs, keyed by job id.
    jobs: RwLock<std::collections::HashMap<String, CancelToken>>,
    /// Installation queue — owned by the core so work survives a window close.
    queue: crate::core::installation::SharedQueue,
    runner: crate::core::installation::runner::InstallRunner,
}

impl AppState {
    pub fn new() -> Self {
        let settings_path = Settings::default_path();
        let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not load settings; using defaults");
            Settings::default()
        });
        let queue = Arc::new(crate::core::installation::InstallQueue::new());
        Self {
            inner: RwLock::new(None),
            settings: RwLock::new(settings),
            settings_path,
            profile_path: crate::core::devices::profile::ProfileStore::default_path(),
            adb: RwLock::new(None),
            jobs: RwLock::new(Default::default()),
            runner: crate::core::installation::runner::InstallRunner::new(queue.clone()),
            queue,
        }
    }

    pub(crate) fn install_runner(&self) -> crate::core::installation::runner::InstallRunner {
        self.runner.clone()
    }

    pub fn queue(&self) -> crate::core::installation::SharedQueue {
        self.queue.clone()
    }

    pub async fn settings(&self) -> Settings {
        self.settings.read().await.clone()
    }

    pub async fn update_settings<F>(&self, f: F) -> CoreResult<Settings>
    where
        F: FnOnce(&mut Settings),
    {
        let mut guard = self.settings.write().await;
        f(&mut guard);
        guard.save(&self.settings_path)?;
        Ok(guard.clone())
    }

    /// Resolve ADB (honouring the configured path) and build the manager.
    /// Called lazily so a missing ADB does not stop the app from starting.
    pub async fn manager(&self) -> CoreResult<Arc<DeviceManager>> {
        if let Some(existing) = self.inner.read().await.clone() {
            return Ok(existing);
        }

        let configured = self.settings.read().await.adb_path.clone();
        let resolved = binary::resolve(configured.as_deref())?;
        tracing::info!(path = %resolved.path.display(), source = ?resolved.source, "using adb");

        let client = AdbClient::new(resolved.path.clone());
        // Start the daemon once, up front; later commands then never pay for it.
        let _ = client.start_server(&CancelToken::new()).await;

        let manager = Arc::new(DeviceManager::new(client, self.profile_path.clone()));
        *self.adb.write().await = Some(resolved);
        *self.inner.write().await = Some(manager.clone());
        Ok(manager)
    }

    /// Drop the cached manager so the next call re-resolves ADB (used after
    /// the user changes the ADB path in Settings).
    pub async fn reset_manager(&self) {
        *self.inner.write().await = None;
        *self.adb.write().await = None;
    }

    pub async fn adb_info(&self) -> Option<AdbBinary> {
        self.adb.read().await.clone()
    }

    // ---- job registry -------------------------------------------------------

    pub async fn register_job(&self, id: &str) -> CancelToken {
        let token = CancelToken::new();
        self.jobs
            .write()
            .await
            .insert(id.to_string(), token.clone());
        token
    }

    pub async fn finish_job(&self, id: &str) {
        self.jobs.write().await.remove(id);
    }

    pub async fn cancel_job(&self, id: &str) -> CoreResult<()> {
        match self.jobs.read().await.get(id) {
            Some(token) => {
                token.cancel();
                Ok(())
            }
            None => Err(CoreError::other(format!("no such job: {id}"))),
        }
    }

    pub async fn active_jobs(&self) -> Vec<String> {
        self.jobs.read().await.keys().cloned().collect()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn job_registry_cancels_and_cleans_up() {
        let state = AppState::new();
        let token = state.register_job("job-1").await;
        assert!(!token.is_cancelled());
        assert_eq!(state.active_jobs().await, vec!["job-1".to_string()]);

        state.cancel_job("job-1").await.unwrap();
        assert!(token.is_cancelled());

        state.finish_job("job-1").await;
        assert!(state.active_jobs().await.is_empty());
        assert!(state.cancel_job("job-1").await.is_err());
    }
}
