//! Hardware integration tests.
//!
//! These run against a REAL `adb` binary and are skipped automatically when
//! none is installed, so CI without the Android SDK stays green. They are the
//! honest proof that the ADB layer actually drives the tool — a real headset
//! is only required for the device-specific assertions at the bottom.
//!
//! Run explicitly with:
//!   cargo test --test adb_integration -- --nocapture --ignored

use std::time::Duration;

use vrmanager_lib::core::adb::binary;
use vrmanager_lib::core::adb::client::AdbClient;
use vrmanager_lib::core::cancel::CancelToken;
use vrmanager_lib::core::platform::detect::{detect_platform, Platform};

/// Locate a real adb on this machine, if any.
fn real_adb() -> Option<std::path::PathBuf> {
    match binary::resolve(None) {
        Ok(found) => {
            println!(
                "[env] adb resolved via {:?}: {}",
                found.source,
                found.path.display()
            );
            Some(found.path)
        }
        Err(e) => {
            println!("[env] no adb available: {e} — integration test skipped");
            None
        }
    }
}

#[tokio::test]
async fn adb_binary_can_be_resolved_on_this_machine() {
    let Some(path) = real_adb() else { return };
    assert!(
        path.is_file(),
        "resolved adb must exist: {}",
        path.display()
    );
}

#[tokio::test]
async fn real_adb_reports_its_version() {
    let Some(path) = real_adb() else { return };
    let client = AdbClient::new(path);
    let cancel = CancelToken::new();

    let version = client
        .version(&cancel)
        .await
        .expect("running `adb version` must succeed");
    println!("[adb] {version}");

    assert!(!version.is_empty());
    assert!(
        version
            .to_ascii_lowercase()
            .contains("android debug bridge")
            || version.to_ascii_lowercase().contains("version"),
        "unexpected version string: {version}"
    );
}

#[tokio::test]
async fn adb_server_starts_and_device_list_parses() {
    let Some(path) = real_adb() else { return };
    let client = AdbClient::new(path);
    let cancel = CancelToken::new();

    client.start_server(&cancel).await.expect("start-server");

    // This is the real parser against real output, empty or not.
    let devices = client
        .list_devices(&cancel)
        .await
        .expect("listing devices must succeed");

    println!("[adb] {} device(s) attached", devices.len());
    for d in &devices {
        println!(
            "[adb]   {} state={:?} network={} model={:?}",
            d.serial,
            d.state,
            d.is_network(),
            d.model_hint()
        );
        // Every entry the real tool prints must parse into a sane shape.
        assert!(!d.serial.is_empty());
        assert!(!d.serial.contains(' '));
    }
}

/// Only runs when a headset is actually plugged in and authorized.
#[tokio::test]
#[ignore = "requires a real Pico/Quest attached over USB with debugging authorized"]
async fn connected_headset_is_identified_and_has_capabilities() {
    let Some(path) = real_adb() else { return };
    let client = AdbClient::new(path);
    let cancel = CancelToken::new();

    let devices = client.list_devices(&cancel).await.unwrap();
    let ready = devices.iter().find(|d| d.state.is_ready());
    let Some(device) = ready else {
        panic!("no authorized device attached — plug in the headset and accept the prompt");
    };

    let props = client
        .getprops(&device.serial, &cancel)
        .await
        .expect("getprop on a real device");

    let platform = detect_platform(&props);
    let adapter = vrmanager_lib::core::platform::adapter::adapter_for(platform);

    println!("[device] serial   : {}", device.serial);
    println!("[device] platform : {platform:?}");
    println!("[device] model    : {}", adapter.display_model(&props));
    println!(
        "[device] android  : {} (sdk {:?})",
        props
            .get("ro.build.version.release")
            .cloned()
            .unwrap_or_default(),
        props.get("ro.build.version.sdk")
    );

    assert_ne!(
        platform,
        Platform::Unknown,
        "a real headset must be identified, not left Unknown"
    );

    // The user-facing label must never be a bare serial.
    let name = adapter.display_model(&props);
    assert!(!name.is_empty());
    assert_ne!(name, device.serial);

    // Installed packages must come back as clean package names.
    let raw = client
        .shell(&device.serial, &["pm", "list", "packages", "-3"], &cancel)
        .await
        .expect("pm list packages");
    let pkgs = vrmanager_lib::core::adb::parse::parse_package_list(&raw);
    println!("[device] {} user apps installed", pkgs.len());
    for (pkg, _) in pkgs.iter().take(10) {
        println!("[device]   {pkg}");
        assert!(!pkg.contains(':'));
        assert!(pkg.contains('.'), "suspicious package name: {pkg}");
    }
}

#[tokio::test]
async fn timeout_is_enforced_on_a_hanging_command() {
    let Some(path) = real_adb() else { return };
    let client = AdbClient::new(path);
    let cancel = CancelToken::new();

    // No such device serial: adb waits/errors, so this exercises the timeout
    // and error paths rather than the happy one.
    let started = std::time::Instant::now();
    let result = client
        .exec_device(
            "ZZZ_NOT_A_REAL_DEVICE",
            &["shell", "echo", "hi"],
            Duration::from_secs(8),
            &cancel,
        )
        .await;
    let elapsed = started.elapsed();

    println!(
        "[adb] unknown-serial result after {:?}: {:?}",
        elapsed,
        result.is_ok()
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "a dead device must not hang the caller ({elapsed:?})"
    );
}
