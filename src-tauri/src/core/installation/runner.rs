//! Window-independent scheduling; production implements Installer with DeviceManager.
use super::{InstallJobSnapshot, ItemStatus, QueuedInstall, SharedQueue};
use crate::core::{apk::ApkMetadata, cancel::CancelToken, devices::manager::InstallOutcome};
use crate::error::{CoreResult, UiError, UiResult};
use crate::events::InstallStage;
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

pub(crate) type InstallFuture<'a> =
    Pin<Box<dyn Future<Output = CoreResult<InstallOutcome>> + Send + 'a>>;
pub(crate) type Publish = Arc<dyn Fn(InstallJobSnapshot) + Send + Sync>;

pub(crate) trait Installer: Send + Sync {
    /// Called only on a blocking worker, once per distinct file in a job.
    fn prepare(&self, path: &Path) -> CoreResult<ApkMetadata>;
    fn install<'a>(
        &'a self,
        serial: &'a str,
        apk: &'a ApkMetadata,
        cancel: &'a CancelToken,
        progress: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a>;
}

#[derive(Clone)]
pub(crate) struct InstallRunner {
    queue: SharedQueue,
    // Shared by clones in AppState, not tied to a replaceable DeviceManager.
    gates: Arc<std::sync::Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>>,
}

impl InstallRunner {
    pub fn new(queue: SharedQueue) -> Self {
        Self {
            queue,
            gates: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    fn gate(&self, serial: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self.gates.lock().unwrap_or_else(|error| error.into_inner());
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(serial).and_then(std::sync::Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        gates.insert(serial.to_owned(), Arc::downgrade(&gate));
        gate
    }

    /// Dropping the returned observer does NOT cancel the owned job.
    pub async fn start(
        &self,
        installer: Arc<dyn Installer>,
        items: Vec<QueuedInstall>,
        publish: Publish,
    ) -> UiResult<(InstallJobSnapshot, tokio::task::JoinHandle<()>)> {
        let (snapshot, cancel) = self.queue.create(items.clone()).await?;
        self.publish(Some(snapshot.clone()), &publish);
        let runner = self.clone();
        let id = snapshot.job_id.clone();
        let worker_runner = self.clone();
        let worker_id = id.clone();
        let worker_items = items.clone();
        let worker_cancel = cancel.clone();
        let worker_publish = publish.clone();
        let worker = tokio::spawn(async move {
            worker_runner
                .drive(
                    installer,
                    &worker_id,
                    &worker_items,
                    &worker_cancel,
                    &worker_publish,
                )
                .await
        });
        // Fixed-size supervision, not one task per progress line. Join errors
        // (including panics) must not leave retained jobs permanently running.
        let finished = tokio::spawn(async move {
            let result = worker
                .await
                .unwrap_or_else(|error| Err(stopped(error.to_string())));
            if let Err(error) = result {
                cancel.cancel();
                runner.fail_remaining(&id, &items, error, &publish).await;
            }
        });
        Ok((snapshot, finished))
    }

    async fn drive(
        &self,
        installer: Arc<dyn Installer>,
        id: &str,
        items: &[QueuedInstall],
        cancel: &CancelToken,
        publish: &Publish,
    ) -> UiResult<()> {
        let paths: Vec<PathBuf> = items.iter().map(|item| item.apk_path.clone()).collect();
        let preparer = installer.clone();
        let preparation_cancel = cancel.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let mut prepared = HashMap::new();
            for path in paths {
                if preparation_cancel.is_cancelled() {
                    break;
                }
                prepared
                    .entry(path.clone())
                    .or_insert_with(|| preparer.prepare(&path).map_err(UiError::from));
            }
            prepared
        })
        .await
        .map_err(|error| stopped(error.to_string()))?;

        // All inspections have finished before the first install can mutate a device.
        for item in items {
            if cancel.is_cancelled() {
                break;
            }
            let Some(result) = prepared.get(&item.apk_path) else {
                return Err(stopped("Prepared APK metadata was unavailable.".into()));
            };
            match result {
                Ok(apk) => self.publish(
                    self.queue
                        .update_item(id, &item.item_id, |state| {
                            state.package = Some(apk.package.clone());
                            state.application_name = Some(apk.display_name());
                        })
                        .await,
                    publish,
                ),
                Err(error) => {
                    self.finish_item(id, item, Err(error.clone()), publish)
                        .await
                }
            }
        }
        for item in items {
            if cancel.is_cancelled() {
                break;
            }
            let Some(Ok(apk)) = prepared.get(&item.apk_path) else {
                continue;
            };
            let _device_guard = tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                guard = self.gate(&item.serial).lock_owned() => guard,
            };
            self.publish(
                self.queue
                    .update_item(id, &item.item_id, |state| {
                        state.status = ItemStatus::Running;
                        state.stage = InstallStage::Preparing;
                    })
                    .await,
                publish,
            );
            let result = self
                .install_item(id, item, apk, installer.as_ref(), cancel, publish)
                .await;
            self.finish_item(id, item, result, publish).await;
        }
        if cancel.is_cancelled() {
            self.publish(self.queue.cancel(id).await, publish);
        }
        Ok(())
    }

    async fn install_item(
        &self,
        id: &str,
        item: &QueuedInstall,
        apk: &ApkMetadata,
        installer: &dyn Installer,
        cancel: &CancelToken,
        publish: &Publish,
    ) -> UiResult<InstallOutcome> {
        // Keep only the latest measured progress; a noisy child cannot create
        // unbounded tasks or make a late progress update overwrite completion.
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        let mut progress = move |line: &str| {
            let (stage, percent) = crate::events::classify_progress(line);
            let measured = if stage == InstallStage::Done {
                (InstallStage::Verifying, None)
            } else {
                (stage, percent)
            };
            tx.send_replace(Some(measured));
        };
        let outcome = {
            let install = installer.install(&item.serial, apk, cancel, &mut progress);
            tokio::pin!(install);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break Err(crate::error::CoreError::Cancelled),
                    changed = rx.changed() => {
                        if changed.is_err() { break Err(crate::error::CoreError::other("Progress channel closed.")); }
                        let measured = *rx.borrow_and_update();
                        self.progress(id, &item.item_id, measured, publish).await;
                    }
                    result = &mut install => break result,
                }
            }
        };
        if rx.has_changed().unwrap_or(false) {
            let measured = *rx.borrow_and_update();
            self.progress(id, &item.item_id, measured, publish).await;
        }
        outcome.map_err(UiError::from)
    }

    async fn progress(
        &self,
        id: &str,
        item: &str,
        measured: Option<(InstallStage, Option<u8>)>,
        publish: &Publish,
    ) {
        if let Some((stage, percent)) = measured {
            self.publish(
                self.queue
                    .update_item(id, item, |state| {
                        state.stage = stage;
                        state.percent = percent;
                    })
                    .await,
                publish,
            );
        }
    }

    async fn finish_item(
        &self,
        id: &str,
        item: &QueuedInstall,
        result: UiResult<InstallOutcome>,
        publish: &Publish,
    ) {
        self.publish(
            self.queue
                .update_item(id, &item.item_id, |state| match result {
                    Ok(outcome) => {
                        state.status = ItemStatus::Succeeded;
                        state.stage = InstallStage::Done;
                        state.percent = Some(100);
                        state.package = outcome.package;
                    }
                    Err(error) => {
                        state.status = if error.kind == "cancelled" {
                            ItemStatus::Cancelled
                        } else {
                            ItemStatus::Failed
                        };
                        state.stage = InstallStage::Failed;
                        state.percent = None;
                        state.error = Some(error);
                    }
                })
                .await,
            publish,
        );
    }

    async fn fail_remaining(
        &self,
        id: &str,
        items: &[QueuedInstall],
        error: UiError,
        publish: &Publish,
    ) {
        for item in items {
            self.finish_item(id, item, Err(error.clone()), publish)
                .await;
        }
    }

    fn publish(&self, snapshot: Option<InstallJobSnapshot>, publish: &Publish) {
        if let Some(snapshot) = snapshot {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| publish(snapshot))).is_err()
            {
                tracing::warn!(
                    "installation state delivery failed; retained state is still available"
                );
            }
        }
    }
}

fn stopped(details: String) -> UiError {
    UiError::new(
        "installRunnerStopped",
        "The installation stopped unexpectedly.",
    )
    .with_details(details)
    .retryable(true)
}
