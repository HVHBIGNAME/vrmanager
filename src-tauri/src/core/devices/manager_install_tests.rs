use super::tests::test_device;
use super::*;
use std::sync::OnceLock;

struct Fixture {
    binary: PathBuf,
    apk: PathBuf,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("vrmanager-install-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join(format!("install-fixture{}", std::env::consts::EXE_SUFFIX));
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/install_fixture.rs");
        let output = std::process::Command::new("rustc")
            .arg("--edition=2021")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let apk = dir.join("app.apk");
        std::fs::write(
            &apk,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/valid-install.apk"
            )),
        )
        .unwrap();
        Fixture { binary, apk }
    })
}

fn apk() -> ApkMetadata {
    ApkMetadata {
        path: fixture().apk.clone(),
        file_name: "app.apk".into(),
        size_bytes: 308,
        package: "com.example.review".into(),
        application_label: Some("Review".into()),
        version_name: Some("1.0".into()),
        version_code: Some(1),
        min_sdk: Some(32),
        target_sdk: Some(32),
        abis: vec!["arm64-v8a".into()],
        permissions: vec![],
        has_icon: false,
        vr_capable: false,
    }
}

async fn device(scenario: &str, streaming: bool) -> DeviceManager {
    let f = fixture();
    let manager = DeviceManager::new(
        AdbClient::new(&f.binary),
        f.binary.with_extension(format!("{scenario}.profile")),
    );
    let mut device = test_device(scenario, &["arm64-v8a"], 32, 1 << 30);
    if streaming {
        device.capabilities.insert(Capability::StreamingInstall);
    }
    manager.cache.write().await.insert(scenario.into(), device);
    manager
}
fn trace(scenario: &str) -> String {
    std::fs::read_to_string(fixture().binary.with_extension(format!("{scenario}.trace"))).unwrap()
}

#[tokio::test]
async fn successful_install_requires_package_readback_without_blanket_permissions() {
    let manager = device("good", false).await;
    let result = manager
        .install_apk("good", &apk(), &CancelToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(result.package.as_deref(), Some("com.example.review"));
    let calls = trace("good");
    assert!(
        calls.contains("'pm' 'path' 'com.example.review'"),
        "package must be read back"
    );
    assert!(
        !calls.split(['|', '\n']).any(|arg| arg == "-g"),
        "permissions must not be granted by default"
    );
}

#[tokio::test]
async fn nonzero_status_and_success_substrings_are_not_install_success() {
    for scenario in ["exit-error", "substring"] {
        let manager = device(scenario, false).await;
        let result = manager
            .install_apk(scenario, &apk(), &CancelToken::new(), |_| {})
            .await;
        assert!(
            matches!(result, Err(CoreError::AdbFailed { .. })),
            "{scenario}: {result:?}"
        );
    }
}

#[tokio::test]
async fn explicit_device_failure_wins_over_success_line() {
    let manager = device("device-failure", true).await;
    let result = manager
        .install_apk("device-failure", &apk(), &CancelToken::new(), |_| {})
        .await;
    assert!(
        matches!(result, Err(CoreError::InstallFailed { ref code, .. }) if code == "INSTALL_FAILED_INVALID_APK"),
        "{result:?}"
    );
    assert_eq!(
        trace("device-failure")
            .lines()
            .filter(|l| l.contains("|install|"))
            .count(),
        1
    );
}

#[tokio::test]
async fn absent_package_after_reported_success_is_a_typed_failure() {
    let manager = device("missing", false).await;
    let error = manager
        .install_apk("missing", &apk(), &CancelToken::new(), |_| {})
        .await
        .unwrap_err();
    let ui: crate::error::UiError = error.into();
    assert_eq!(ui.kind, "installNotVerified");
    assert!(ui.retryable);
}

#[tokio::test]
async fn failed_readback_is_not_install_success() {
    let manager = device("read-error", false).await;
    let result = manager
        .install_apk("read-error", &apk(), &CancelToken::new(), |_| {})
        .await;
    assert!(
        matches!(result, Err(CoreError::AdbFailed { code: Some(9), .. })),
        "{result:?}"
    );
}

#[tokio::test]
async fn unsupported_streaming_falls_back_once_then_verifies() {
    let manager = device("fallback", true).await;
    let outcome = manager
        .install_apk("fallback", &apk(), &CancelToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.method, "standard");
    assert_eq!(outcome.package.as_deref(), Some("com.example.review"));
    assert_eq!(
        trace("fallback")
            .lines()
            .filter(|l| l.contains("|install|"))
            .count(),
        2
    );
}

#[tokio::test]
async fn cancellation_never_starts_a_fallback_attempt() {
    let manager = device("cancel", true).await;
    let token = CancelToken::new();
    let cancel = token.clone();
    let result = manager
        .install_apk("cancel", &apk(), &token, move |line| {
            if line.contains("INSTALL_STARTED") {
                cancel.cancel();
            }
        })
        .await;
    assert!(matches!(result, Err(CoreError::Cancelled)), "{result:?}");
    assert_eq!(
        trace("cancel")
            .lines()
            .filter(|l| l.contains("|install|"))
            .count(),
        1
    );
}
