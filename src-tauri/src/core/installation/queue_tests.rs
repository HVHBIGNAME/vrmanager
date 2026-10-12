use super::*;

fn items(pairs: &[(&str, &str)]) -> Vec<QueuedInstall> {
    pairs
        .iter()
        .enumerate()
        .map(|(i, (serial, path))| QueuedInstall {
            item_id: format!("item-{i}"),
            serial: serial.to_string(),
            apk_path: std::env::temp_dir().join(path),
        })
        .collect()
}

#[tokio::test]
async fn job_expands_into_one_item_per_pair() {
    let queue = InstallQueue::new();
    let (snap, _) = queue
        .create(items(&[("S1", "a.apk"), ("S2", "a.apk")]))
        .await
        .unwrap();
    assert_eq!(snap.items.len(), 2);
    assert_eq!(snap.status, JobStatus::Queued);
    assert_eq!(snap.revision, 1);
}

#[tokio::test]
async fn one_failure_makes_the_whole_job_failed() {
    let queue = InstallQueue::new();
    let (snap, _) = queue
        .create(items(&[("S1", "a.apk"), ("S2", "a.apk")]))
        .await
        .unwrap();
    queue
        .update_item(&snap.job_id, "item-0", |i| {
            i.status = ItemStatus::Failed;
            i.error = Some(UiError::new(
                "installFailed",
                "The device rejected the APK.",
            ));
        })
        .await
        .unwrap();
    let final_snap = queue
        .update_item(&snap.job_id, "item-1", |i| {
            i.status = ItemStatus::Succeeded;
        })
        .await
        .unwrap();
    assert_eq!(final_snap.status, JobStatus::Failed);
}

#[tokio::test]
async fn revision_is_monotonic_so_stale_snapshots_are_detectable() {
    let queue = InstallQueue::new();
    let (snap, _) = queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    let after = queue
        .update_item(&snap.job_id, "item-0", |i| i.percent = Some(10))
        .await
        .unwrap();
    let later = queue
        .update_item(&snap.job_id, "item-0", |i| i.percent = Some(90))
        .await
        .unwrap();
    assert!(after.revision > snap.revision);
    assert!(later.revision > after.revision);
}

#[tokio::test]
async fn cancel_terminalises_pending_items_and_trips_the_token() {
    let queue = InstallQueue::new();
    let (snap, token) = queue
        .create(items(&[("S1", "a.apk"), ("S2", "a.apk")]))
        .await
        .unwrap();
    queue
        .update_item(&snap.job_id, "item-0", |i| i.status = ItemStatus::Succeeded)
        .await
        .unwrap();
    let cancelled = queue.cancel(&snap.job_id).await.unwrap();
    assert!(token.is_cancelled());
    assert_eq!(cancelled.items[0].status, ItemStatus::Succeeded);
    assert_eq!(cancelled.items[1].status, ItemStatus::Cancelled);
    assert_eq!(cancelled.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn retry_covers_only_eligible_items_not_the_whole_batch() {
    let queue = InstallQueue::new();
    let (snap, _) = queue
        .create(items(&[("S1", "a.apk"), ("S2", "a.apk"), ("S3", "a.apk")]))
        .await
        .unwrap();
    queue
        .update_item(&snap.job_id, "item-0", |i| i.status = ItemStatus::Succeeded)
        .await
        .unwrap();
    queue
        .update_item(&snap.job_id, "item-1", |i| {
            i.status = ItemStatus::Failed;
            i.error = Some(UiError::new("installFailed", "Different processor."));
        })
        .await
        .unwrap();
    queue
        .update_item(&snap.job_id, "item-2", |i| {
            i.status = ItemStatus::Failed;
            i.error = Some(UiError::new("installFailed", "Device was busy.").retryable(true));
        })
        .await
        .unwrap();
    let retry = queue.retryable_items(&snap.job_id).await;
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].serial, "S3");
}

#[tokio::test]
async fn snapshot_survives_for_a_ui_that_reconnects_later() {
    let queue = InstallQueue::new();
    let (snap, _) = queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    queue
        .update_item(&snap.job_id, "item-0", |i| i.status = ItemStatus::Succeeded)
        .await
        .unwrap();
    let listed = queue.list().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, JobStatus::Completed);
    assert!(queue.get(&snap.job_id).await.is_some());
}

#[tokio::test]
async fn jobs_are_isolated_from_each_other() {
    let queue = InstallQueue::new();
    let (a, _) = queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    let (b, _) = queue.create(items(&[("S1", "b.apk")])).await.unwrap();
    queue
        .update_item(&a.job_id, "item-0", |i| i.status = ItemStatus::Failed)
        .await
        .unwrap();
    assert_eq!(
        queue.get(&b.job_id).await.unwrap().status,
        JobStatus::Queued
    );
}

#[tokio::test]
async fn unknown_job_is_reported_not_guessed() {
    let queue = InstallQueue::new();
    assert!(queue.get("nope").await.is_none());
    assert!(queue.cancel("nope").await.is_none());
    assert!(queue.retryable_items("nope").await.is_empty());
}
