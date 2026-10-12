use super::*;
use crate::events::InstallStage;

fn items(pairs: &[(&str, &str)]) -> Vec<QueuedInstall> {
    pairs
        .iter()
        .enumerate()
        .map(|(n, (serial, path))| QueuedInstall {
            item_id: format!("item-{n}"),
            serial: serial.to_string(),
            apk_path: std::env::temp_dir().join(path),
        })
        .collect()
}

#[tokio::test]
async fn malformed_pairs_are_rejected_without_silent_identifier_repair() {
    let queue = InstallQueue::new();
    for serial in ["", " S1", "S1\n", "-s", "S;reboot"] {
        assert!(
            queue.create(items(&[(serial, "a.apk")])).await.is_err(),
            "accepted {serial:?}"
        );
    }
    for path in ["a.apk", "Z:/bad\0.apk", ""] {
        let mut pair = items(&[("S1", "a.apk")]);
        pair[0].apk_path = PathBuf::from(path);
        assert!(queue.create(pair).await.is_err(), "accepted {path:?}");
    }
    let mut pairs = items(&[("S1", "a.apk"), ("S2", "a.apk")]);
    pairs[1].item_id = pairs[0].item_id.clone();
    assert!(
        queue.create(pairs).await.is_err(),
        "item IDs must be unique"
    );
    assert!(queue.list().await.is_empty());
}

#[tokio::test]
async fn job_item_and_active_capacity_limits_are_enforced() {
    let queue = InstallQueue::new();
    let oversized: Vec<_> = (0..513)
        .map(|n| QueuedInstall {
            item_id: format!("item-{n}"),
            serial: "S1".into(),
            apk_path: PathBuf::from(format!("Z:/{n}.apk")),
        })
        .collect();
    assert!(
        queue.create(oversized).await.is_err(),
        "one call must not allocate an unbounded job"
    );
    for _ in 0..16 {
        queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    }
    assert!(
        queue.create(items(&[("S1", "a.apk")])).await.is_err(),
        "active job admission must be bounded atomically"
    );
    let old = queue.list().await[0].job_id.clone();
    queue.cancel(&old).await.unwrap();
    assert!(
        queue.create(items(&[("S1", "a.apk")])).await.is_ok(),
        "terminal history must not consume active capacity"
    );
}

#[tokio::test]
async fn empty_jobs_are_rejected_before_registry_mutation() {
    let queue = InstallQueue::new();
    let _ = queue.create(Vec::new()).await;
    assert!(
        queue.list().await.is_empty(),
        "empty jobs must not occupy registry or active tokens"
    );
}

#[tokio::test]
async fn retry_requires_terminal_job_and_preserves_only_eligible_pairs() {
    let queue = InstallQueue::new();
    let (job, _) = queue
        .create(items(&[
            ("S1", "a.apk"),
            ("S2", "a.apk"),
            ("S1", "b.apk"),
            ("S2", "b.apk"),
            ("S3", "b.apk"),
        ]))
        .await
        .unwrap();
    queue
        .update_item(&job.job_id, "item-0", |i| {
            i.status = ItemStatus::Failed;
            i.error = Some(UiError::new("timeout", "Retry me.").retryable(true));
        })
        .await;
    assert!(
        queue.retryable_items(&job.job_id).await.is_empty(),
        "still-active jobs cannot retry early failures"
    );
    queue
        .update_item(&job.job_id, "item-1", |i| i.status = ItemStatus::Succeeded)
        .await;
    queue
        .update_item(&job.job_id, "item-2", |i| i.status = ItemStatus::Failed)
        .await;
    queue
        .update_item(&job.job_id, "item-3", |i| i.status = ItemStatus::Cancelled)
        .await;
    queue
        .update_item(&job.job_id, "item-4", |i| {
            i.status = ItemStatus::Failed;
            i.error = Some(UiError::new("apkInvalid", "Do not retry."));
        })
        .await;
    let retry = queue.retryable_items(&job.job_id).await;
    let pairs: Vec<_> = retry
        .iter()
        .map(|i| {
            (
                i.serial.as_str(),
                i.apk_path.file_name().unwrap().to_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(pairs, [("S1", "a.apk"), ("S2", "b.apk")]);
    assert!(retry
        .iter()
        .all(|i| job.items.iter().all(|j| i.item_id != j.item_id)));
}

#[tokio::test]
async fn terminal_transitions_bound_history_and_remove_active_tokens() {
    let queue = InstallQueue {
        history_limit: 2,
        ..InstallQueue::new()
    };
    let (active, _) = queue.create(items(&[("S0", "a.apk")])).await.unwrap();
    let mut jobs = Vec::new();
    for _ in 0..4 {
        jobs.push(queue.create(items(&[("S1", "a.apk")])).await.unwrap().0);
    }
    for job in &jobs {
        queue
            .update_item(&job.job_id, "item-0", |i| i.status = ItemStatus::Succeeded)
            .await;
    }
    let list = queue.list().await;
    assert_eq!(
        list.len(),
        3,
        "keep two terminal jobs plus all active jobs, without another create"
    );
    assert!(queue.get(&active.job_id).await.is_some());
    assert!(queue.get(&jobs[0].job_id).await.is_none());
    assert!(queue.cancel_token(&jobs[3].job_id).await.is_none());
    let before = queue.get(&jobs[3].job_id).await.unwrap();
    let after = queue.cancel(&jobs[3].job_id).await.unwrap();
    assert_eq!(before.revision, after.revision);
    assert_eq!(after.status, JobStatus::Completed);
}

#[tokio::test]
async fn list_and_prune_cannot_form_a_lock_cycle() {
    use std::future::Future;
    use std::task::Poll;
    let queue = InstallQueue::new();
    queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    let list = queue.list();
    tokio::pin!(list);
    // Exhaust one poll's cooperative budget exactly between the two reads.
    tokio::task::yield_now().await;
    for _ in 0..127 {
        tokio::task::consume_budget().await;
    }
    let list_done = std::future::poll_fn(|cx| Poll::Ready(list.as_mut().poll(cx).is_ready())).await;
    let prune = tokio::task::unconstrained(queue.prune());
    tokio::pin!(prune);
    let prune_done =
        std::future::poll_fn(|cx| Poll::Ready(prune.as_mut().poll(cx).is_ready())).await;
    let result = tokio::time::timeout(std::time::Duration::from_millis(30), async {
        tokio::join!(
            async {
                if !list_done {
                    list.await;
                }
            },
            async {
                if !prune_done {
                    prune.await;
                }
            }
        )
    })
    .await;
    assert!(
        result.is_ok(),
        "list holds jobs waiting for order; prune holds order waiting for jobs"
    );
}

#[tokio::test]
async fn repeated_cancel_preserves_terminal_revision_and_releases_token() {
    let queue = InstallQueue::new();
    let (job, token) = queue.create(items(&[("S1", "a.apk")])).await.unwrap();
    let first = queue.cancel(&job.job_id).await.unwrap();
    let second = queue.cancel(&job.job_id).await.unwrap();
    assert_eq!(second.revision, first.revision, "cancel must be idempotent");
    assert!(token.is_cancelled());
    assert!(
        queue.cancel_token(&job.job_id).await.is_none(),
        "terminal token leaked"
    );
    assert!(queue.cancel("missing").await.is_none());
}

#[tokio::test]
async fn terminal_items_ignore_late_progress_and_completion() {
    let queue = InstallQueue::new();
    let (job, _) = queue
        .create(items(&[("S1", "a.apk"), ("S2", "a.apk")]))
        .await
        .unwrap();
    let cancelled = queue.cancel(&job.job_id).await.unwrap();
    let update = queue
        .update_item(&job.job_id, "item-0", |item| {
            item.stage = InstallStage::Transferring;
            item.percent = Some(50);
            item.status = ItemStatus::Succeeded;
        })
        .await;
    assert!(
        update.is_none(),
        "late callbacks must not mutate or emit a terminal item"
    );
    assert_eq!(
        serde_json::to_value(queue.get(&job.job_id).await).unwrap(),
        serde_json::to_value(Some(cancelled)).unwrap()
    );
}
