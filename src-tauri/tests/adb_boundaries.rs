//! Subprocess boundary tests with a compiled, test-only fake executable.
use std::{path::PathBuf, process::Command, sync::OnceLock};
use vrmanager_lib::{
    core::{adb::client::AdbClient, cancel::CancelToken},
    CoreError,
};

fn fixture() -> &'static PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("vrmanager-adb-boundaries-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join(format!(
            "adb-boundary-fixture{}",
            std::env::consts::EXE_SUFFIX
        ));
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/adb_boundary_fixture.rs");
        let output = Command::new("rustc")
            .arg("--edition=2021")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("compile fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}

fn assert_status_failure<T: std::fmt::Debug>(result: Result<T, CoreError>) {
    match result {
        Err(CoreError::AdbFailed { code, stderr, .. }) => {
            assert_eq!(code, Some(23));
            assert!(stderr.contains("protocol fault"));
        }
        other => panic!("nonzero ADB exit must not become success: {other:?}"),
    }
}

#[tokio::test]
async fn control_helpers_preserve_nonzero_exit_status() {
    let client = AdbClient::new(fixture());
    let cancel = CancelToken::new();
    assert_status_failure(client.start_server(&cancel).await);
    assert_status_failure(client.disconnect("localhost:5555", &cancel).await);
    assert_status_failure(client.connect("localhost:5555", &cancel).await);
    assert_status_failure(client.tcpip("S1", 5555, &cancel).await);
}

#[tokio::test]
async fn shell_serialization_preserves_literal_arguments() {
    let client = AdbClient::new(fixture());
    let output = client
        .shell(
            "S1",
            &[
                "printf",
                "%s\\n",
                "alpha beta",
                "quote'piece",
                "$HOME; literal",
                "",
            ],
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        output,
        "'printf' '%s\\n' 'alpha beta' 'quote'\\''piece' '$HOME; literal' ''\n"
    );
}

#[tokio::test]
async fn platform_launch_uses_the_literal_shell_boundary() {
    use vrmanager_lib::core::platform::{AndroidAdapter, PlatformAdapter};
    let client = AdbClient::new(fixture());
    AndroidAdapter
        .launch(
            &client,
            "adapter-test",
            "com.example.app",
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let source = std::fs::read_to_string(fixture().with_extension("capture.txt")).unwrap();
    assert_eq!(
        source,
        "'monkey' '-p' 'com.example.app' '-c' 'android.intent.category.LAUNCHER' '1'"
    );
}

#[tokio::test]
async fn platform_launch_rejects_invalid_package_before_spawn() {
    use vrmanager_lib::core::platform::{AndroidAdapter, PlatformAdapter};
    let client = AdbClient::new("missing-adb-boundary-fixture");
    let error = AndroidAdapter
        .launch(&client, "S1", "com.example;literal", &CancelToken::new())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("package"),
        "expected package validation, got {error:?}"
    );
}
