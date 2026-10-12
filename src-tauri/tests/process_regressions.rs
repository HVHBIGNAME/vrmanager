//! Test-only subprocess fixtures: never connects to ADB or physical devices.
use std::io::Write;
use std::time::Duration;
use vrmanager_lib::core::adb::client::AdbClient;
use vrmanager_lib::core::cancel::CancelToken;

#[test]
#[ignore = "subprocess fixture; invoked only by streaming_stderr_pressure"]
fn pipe_pressure_fixture() {
    let mut stderr = std::io::stderr().lock();
    for _ in 0..64 {
        writeln!(stderr, "{}", "x".repeat(4096)).expect("write fixture stderr");
    }
    stderr.flush().expect("flush fixture stderr");
    println!("STDOUT_AFTER_STDERR_DRAINED");
}

#[tokio::test]
async fn streaming_stderr_pressure() {
    let executable = std::env::current_exe().expect("test executable");
    let client = AdbClient::new(executable);
    let result = client
        .exec_streaming(
            &[
                "--ignored",
                "--exact",
                "pipe_pressure_fixture",
                "--nocapture",
            ],
            Duration::from_secs(4),
            &CancelToken::new(),
            |_| {},
        )
        .await;
    assert!(
        result.is_ok(),
        "stdout and stderr must drain independently: {result:?}"
    );
    let output = result.expect("concurrent stream capture");
    assert!(output.success());
    assert!(output.stdout.contains("STDOUT_AFTER_STDERR_DRAINED"));
    assert!(output.stderr.len() >= 262144);
}

#[test]
#[ignore = "test-only subprocess used by streaming_preserves_non_utf8_suffix"]
fn non_utf8_fixture() {
    std::io::stdout()
        .write_all(b"before\n\xff\nFailure [INSTALL_FAILED_INVALID_APK]\n")
        .unwrap();
}

#[tokio::test]
async fn streaming_preserves_non_utf8_suffix() {
    let client = AdbClient::new(std::env::current_exe().unwrap());
    let output = client
        .exec_streaming(
            &["--ignored", "--exact", "non_utf8_fixture", "--nocapture"],
            Duration::from_secs(4),
            &CancelToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert!(
        output
            .stdout
            .contains("Failure [INSTALL_FAILED_INVALID_APK]"),
        "diagnostic suffix was lost: {output:?}"
    );
    assert!(output.stdout.contains('\u{fffd}'));
}

#[test]
#[ignore = "short-lived inherited-pipe fixture, never a device process"]
fn pipe_keeper_fixture() {
    std::thread::sleep(Duration::from_secs(3));
    std::process::exit(0);
}

#[test]
#[ignore = "test-only child with an inherited-pipe descendant"]
#[allow(clippy::zombie_processes)] // the descendant is meant to outlive the test child
fn pipe_tree_fixture() {
    let _keeper = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "pipe_keeper_fixture", "--nocapture"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();
    println!("TREE_READY");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(5));
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_stream_future_aborts_readers_even_with_inherited_pipes() {
    let runtime = tokio::runtime::Handle::current();
    let baseline = runtime.metrics().num_alive_tasks();
    let client = AdbClient::new(std::env::current_exe().unwrap());
    let token = CancelToken::new();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let mut ready_tx = Some(ready_tx);
    let args = ["--ignored", "--exact", "pipe_tree_fixture", "--nocapture"];
    let mut call =
        Box::pin(
            client.exec_streaming(&args, Duration::from_secs(10), &token, move |line| {
                if line.contains("TREE_READY") {
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
                    }
                }
            }),
        );
    tokio::select! {
        output = &mut call => panic!("fixture ended before drop: {output:?}"),
        ready = tokio::time::timeout(Duration::from_secs(4), ready_rx) => { ready.unwrap().unwrap(); }
    }
    drop(call);
    let cleaned = tokio::time::timeout(Duration::from_millis(500), async {
        while runtime.metrics().num_alive_tasks() != baseline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        cleaned.is_ok(),
        "reader tasks survived dropped owner: {}",
        runtime.metrics().num_alive_tasks()
    );
}
