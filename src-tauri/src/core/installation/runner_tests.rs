use super::runner::{InstallFuture, InstallRunner, Installer, Publish};
use super::*;
use crate::core::{apk::ApkMetadata, cancel::CancelToken, devices::manager::InstallOutcome};
use crate::error::{CoreError, CoreResult};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

fn metadata(path: &Path) -> ApkMetadata {
    ApkMetadata {
        path: path.to_owned(),
        file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
        size_bytes: 1,
        package: "com.example.app".into(),
        application_label: Some("Example".into()),
        version_name: None,
        version_code: None,
        min_sdk: None,
        target_sdk: None,
        abis: vec![],
        permissions: vec![],
        has_icon: false,
        vr_capable: false,
    }
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<QueuedInstall> {
    pairs
        .iter()
        .enumerate()
        .map(|(i, (serial, file))| QueuedInstall {
            item_id: format!("item-{i}"),
            serial: (*serial).into(),
            apk_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file),
        })
        .collect()
}

#[derive(Default)]
struct RecordingInstaller {
    calls: Mutex<Vec<String>>,
    prepare_threads: Mutex<Vec<std::thread::ThreadId>>,
}
impl Installer for RecordingInstaller {
    fn prepare(&self, path: &Path) -> CoreResult<ApkMetadata> {
        let file = path.file_name().unwrap().to_str().unwrap();
        self.calls.lock().unwrap().push(format!("prepare:{file}"));
        self.prepare_threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        if file == "bad.apk" {
            return Err(CoreError::Apk("invalid fixture".into()));
        }
        Ok(metadata(path))
    }
    fn install<'a>(
        &'a self,
        serial: &'a str,
        apk: &'a ApkMetadata,
        _: &'a CancelToken,
        _: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(format!("install:{}:{serial}", apk.file_name));
            Ok(InstallOutcome {
                package: Some(apk.package.clone()),
                serial: serial.into(),
                method: "test-only",
                duration_ms: 0,
            })
        })
    }
}

struct BlockingInstaller {
    started: tokio::sync::mpsc::UnboundedSender<String>,
    release: tokio::sync::Semaphore,
}
impl Installer for BlockingInstaller {
    fn prepare(&self, path: &Path) -> CoreResult<ApkMetadata> {
        Ok(metadata(path))
    }
    fn install<'a>(
        &'a self,
        serial: &'a str,
        apk: &'a ApkMetadata,
        cancel: &'a CancelToken,
        _: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a> {
        Box::pin(async move {
            self.started.send(serial.into()).unwrap();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
                permit = self.release.acquire() => permit.unwrap().forget(),
            }
            Ok(InstallOutcome {
                package: Some(apk.package.clone()),
                serial: serial.into(),
                method: "test-only",
                duration_ms: 0,
            })
        })
    }
}

#[tokio::test]
async fn same_serial_jobs_serialize_but_another_serial_can_run() {
    use tokio::time::{timeout, Duration};
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue);
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let installer = Arc::new(BlockingInstaller {
        started,
        release: tokio::sync::Semaphore::new(0),
    });
    let (_, a) = runner
        .start(
            installer.clone(),
            pairs(&[("S1", "a.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap()
            .unwrap(),
        "S1"
    );
    let (_, b) = runner
        .start(
            installer.clone(),
            pairs(&[("S1", "b.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let (_, c) = runner
        .start(
            installer.clone(),
            pairs(&[("S2", "c.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap()
            .unwrap(),
        "S2",
        "second S1 job must wait while S2 is independent"
    );
    assert!(
        timeout(Duration::from_millis(50), starts.recv())
            .await
            .is_err(),
        "S1 must still be locked"
    );
    installer.release.add_permits(3);
    timeout(Duration::from_secs(2), async {
        a.await.unwrap();
        b.await.unwrap();
        c.await.unwrap();
    })
    .await
    .unwrap();
    assert_eq!(starts.recv().await.unwrap(), "S1");
}

#[tokio::test]
async fn prepares_every_unique_apk_off_thread_before_installing_ordered_pairs() {
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue.clone());
    let installer = Arc::new(RecordingInstaller::default());
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let publish: Publish = Arc::new(move |snapshot| sink.lock().unwrap().push(snapshot));
    let caller = std::thread::current().id();
    let (initial, finished) = runner
        .start(
            installer.clone(),
            pairs(&[
                ("S2", "a.apk"),
                ("S1", "a.apk"),
                ("S2", "bad.apk"),
                ("S1", "b.apk"),
            ]),
            publish,
        )
        .await
        .unwrap();
    finished.await.unwrap();
    assert_eq!(
        *installer.calls.lock().unwrap(),
        [
            "prepare:a.apk",
            "prepare:bad.apk",
            "prepare:b.apk",
            "install:a.apk:S2",
            "install:a.apk:S1",
            "install:b.apk:S1",
        ]
    );
    assert!(installer
        .prepare_threads
        .lock()
        .unwrap()
        .iter()
        .all(|id| *id != caller));
    let snapshot = queue.get(&initial.job_id).await.unwrap();
    assert_eq!(snapshot.status, JobStatus::Failed);
    assert_eq!(
        snapshot.items.iter().map(|i| i.status).collect::<Vec<_>>(),
        [
            ItemStatus::Succeeded,
            ItemStatus::Succeeded,
            ItemStatus::Failed,
            ItemStatus::Succeeded
        ]
    );
    assert_eq!(
        snapshot.items[0].application_name.as_deref(),
        Some("Example")
    );
    assert_eq!(snapshot.items[2].error.as_ref().unwrap().kind, "apkInvalid");
    assert!(queue.cancel_token(&initial.job_id).await.is_none());
    let events = events.lock().unwrap();
    assert_eq!(events.first().unwrap().revision, initial.revision);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].revision < pair[1].revision));
    assert_eq!(events.last().unwrap().revision, snapshot.revision);
}

struct ProgressInstaller;
impl Installer for ProgressInstaller {
    fn prepare(&self, path: &Path) -> CoreResult<ApkMetadata> {
        Ok(metadata(path))
    }
    fn install<'a>(
        &'a self,
        serial: &'a str,
        apk: &'a ApkMetadata,
        _: &'a CancelToken,
        progress: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a> {
        Box::pin(async move {
            progress("[ 25%] transferring");
            tokio::task::yield_now().await;
            progress("Success");
            tokio::task::yield_now().await;
            Ok(InstallOutcome {
                package: Some(apk.package.clone()),
                serial: serial.into(),
                method: "test-only",
                duration_ms: 0,
            })
        })
    }
}

#[tokio::test]
async fn progress_is_revisioned_before_completion_and_success_waits_for_verification() {
    use crate::events::InstallStage;
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let (_, done) = runner
        .start(
            Arc::new(ProgressInstaller),
            pairs(&[("S1", "a.apk")]),
            Arc::new(move |s| captured.lock().unwrap().push(s)),
        )
        .await
        .unwrap();
    done.await.unwrap();
    let events = events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|s| s.items[0].status == ItemStatus::Running && s.items[0].percent == Some(25)),
        "live progress must reach the retained snapshots"
    );
    assert!(
        events
            .iter()
            .any(|s| s.items[0].status == ItemStatus::Running
                && s.items[0].stage == InstallStage::Verifying
                && s.items[0].percent.is_none()),
        "ADB Success alone is not verified completion"
    );
    assert!(events.windows(2).all(|w| w[0].revision < w[1].revision));
    let last = events.last().unwrap();
    assert_eq!(last.status, JobStatus::Completed);
    assert_eq!(last.items[0].stage, InstallStage::Done);
    assert_eq!(last.items[0].percent, Some(100));
}

struct PanickingInstaller;
impl Installer for PanickingInstaller {
    fn prepare(&self, path: &Path) -> CoreResult<ApkMetadata> {
        Ok(metadata(path))
    }
    fn install<'a>(
        &'a self,
        _: &'a str,
        _: &'a ApkMetadata,
        _: &'a CancelToken,
        _: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a> {
        Box::pin(async { panic!("intentional installer fixture panic") })
    }
}

#[tokio::test]
async fn installer_panic_terminalizes_remaining_items_and_releases_token() {
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue.clone());
    let (initial, done) = runner
        .start(
            Arc::new(PanickingInstaller),
            pairs(&[("S1", "a.apk"), ("S2", "b.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let _ = done.await;
    let final_state = queue.get(&initial.job_id).await.unwrap();
    assert_eq!(
        final_state.status,
        JobStatus::Failed,
        "panic left work active"
    );
    assert!(final_state
        .items
        .iter()
        .all(|i| i.status == ItemStatus::Failed));
    assert!(queue.cancel_token(&initial.job_id).await.is_none());
}

#[tokio::test]
async fn publisher_failure_cannot_prevent_the_owned_job_from_running() {
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue.clone());
    let (initial, done) = runner
        .start(
            Arc::new(RecordingInstaller::default()),
            pairs(&[("S1", "a.apk")]),
            Arc::new(|_| panic!("intentional publisher fixture panic")),
        )
        .await
        .expect("state ownership must not depend on delivery");
    done.await.unwrap();
    assert_eq!(
        queue.get(&initial.job_id).await.unwrap().status,
        JobStatus::Completed
    );
    assert!(queue.cancel_token(&initial.job_id).await.is_none());
}

#[tokio::test]
async fn cancelled_waiting_job_never_starts_and_dropped_observer_does_not_stop_work() {
    use tokio::time::{timeout, Duration};
    let queue = Arc::new(InstallQueue::new());
    let runner = InstallRunner::new(queue.clone());
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let installer = Arc::new(BlockingInstaller {
        started,
        release: tokio::sync::Semaphore::new(0),
    });
    let (a, observer) = runner
        .start(
            installer.clone(),
            pairs(&[("S1", "a.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    drop(observer); // Equivalent to closing the progress view.
    assert_eq!(
        timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap()
            .unwrap(),
        "S1"
    );
    let (b, done) = runner
        .start(
            installer.clone(),
            pairs(&[("S1", "b.apk")]),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    queue.cancel(&b.job_id).await.unwrap();
    timeout(Duration::from_secs(2), done)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.get(&b.job_id).await.unwrap().status,
        JobStatus::Cancelled
    );
    assert!(queue.cancel_token(&b.job_id).await.is_none());
    installer.release.add_permits(1);
    timeout(Duration::from_secs(2), async {
        while !queue.get(&a.job_id).await.unwrap().is_terminal() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        queue.get(&a.job_id).await.unwrap().status,
        JobStatus::Completed
    );
    assert!(
        starts.try_recv().is_err(),
        "cancelled queued pair must never touch the device"
    );
}
