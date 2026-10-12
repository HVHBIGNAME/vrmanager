use super::*;

#[tokio::test]
async fn inspection_rejects_empty_or_oversized_input_and_deduplicates() {
    assert!(
        inspect_apk_batch(Vec::new()).await.is_err(),
        "empty inspection must fail"
    );
    assert!(
        inspect_apks(Vec::new()).await.is_err(),
        "legacy command must use same boundary"
    );
    assert!(
        inspect_apk_batch(vec!["Z:/missing.apk".into(); 65])
            .await
            .is_err(),
        "bound raw input before deduplication"
    );
    assert!(inspect_apk_batch(vec!["relative.apk".into()])
        .await
        .is_err());
    assert!(inspect_apk_batch(vec!["Z:/bad\0.apk".into()])
        .await
        .is_err());
    let batch = inspect_apk_batch(vec!["Z:/missing.apk".into(), "Z:/missing.apk".into()])
        .await
        .unwrap();
    assert!(batch.apks.is_empty());
    assert_eq!(
        batch.errors.len(),
        1,
        "duplicate path must be prepared once"
    );
    assert_eq!(batch.errors[0].path, "Z:/missing.apk");
    assert_eq!(batch.errors[0].error.kind, "apkInvalid");
}
