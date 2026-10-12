//! Device manager: the orchestration layer the Tauri commands talk to.
//!
//! Responsibilities:
//!   * discover devices (USB + network) and enrich them with real facts,
//!   * keep user profiles (custom names) attached,
//!   * expose app management operations through the platform adapter,
//!   * never block: everything is async and cancellable.

#[cfg(test)]
#[path = "manager_install_tests.rs"]
mod install_tests;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::core::adb::client::{AdbClient, DEFAULT_TIMEOUT, LONG_TIMEOUT};
use crate::core::adb::parse::{self, AdbDeviceEntry, AdbDeviceState};
use crate::core::apk::ApkMetadata;
use crate::core::cancel::CancelToken;
use crate::core::devices::model::{ConnectionType, Device, DeviceInfo};
use crate::core::devices::profile::ProfileStore;
use crate::core::platform::adapter::{adapter_for, PlatformAdapter};
use crate::core::platform::capability::Capability;
use crate::core::platform::detect::{detect_platform, Platform};
use crate::error::{CoreError, CoreResult};

/// An installed application as read from a device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub package: String,
    pub apk_path: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<u64>,
    /// Marked when the platform adapter considers it vendor/system noise.
    pub system: bool,
}

/// Outcome of an install attempt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub package: Option<String>,
    pub serial: String,
    /// Which mechanism actually ran (`streaming` or `standard`); the user
    /// never sees this, it exists for logs and support.
    pub method: &'static str,
    pub duration_ms: u128,
}

pub struct DeviceManager {
    client: AdbClient,
    profiles: RwLock<ProfileStore>,
    profile_path: PathBuf,
    /// Cache of enriched devices keyed by serial.
    cache: RwLock<HashMap<String, Device>>,
}

impl DeviceManager {
    pub fn new(client: AdbClient, profile_path: PathBuf) -> Self {
        let profiles = ProfileStore::load(&profile_path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not load device profiles; starting empty");
            ProfileStore::new()
        });
        Self {
            client,
            profiles: RwLock::new(profiles),
            profile_path,
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn client(&self) -> &AdbClient {
        &self.client
    }

    /// List devices with full details. This is the authoritative scan; the UI
    /// calls it on demand and on ADB events rather than on a timer.
    pub async fn list_devices(&self, cancel: &CancelToken) -> CoreResult<Vec<Device>> {
        let entries = self.client.list_devices(cancel).await?;
        let mut out = Vec::with_capacity(entries.len());

        for entry in entries {
            match self.enrich(&entry, cancel).await {
                Ok(device) => out.push(device),
                Err(e) => {
                    // A device that fails enrichment (unauthorized, offline)
                    // must still appear, otherwise the user cannot act on it.
                    tracing::debug!(serial = %entry.serial, error = %e, "partial device");
                    out.push(self.minimal_device(&entry).await);
                }
            }
        }

        {
            let mut cache = self.cache.write().await;
            cache.clear();
            for d in &out {
                cache.insert(d.serial.clone(), d.clone());
            }
        }

        // Stable, human-friendly ordering: ready devices first, then by name.
        out.sort_by(|a, b| {
            b.is_ready().cmp(&a.is_ready()).then_with(|| {
                a.display_name()
                    .to_lowercase()
                    .cmp(&b.display_name().to_lowercase())
            })
        });
        Ok(out)
    }

    /// Device that could not be interrogated (unauthorized/offline).
    async fn minimal_device(&self, entry: &AdbDeviceEntry) -> Device {
        let model_hint = entry
            .model_hint()
            .map(|m| m.replace('_', " "))
            .unwrap_or_else(|| "Android device".to_string());
        let custom_name = self.profiles.read().await.name_for(&entry.serial);
        Device {
            serial: entry.serial.clone(),
            custom_name,
            model_name: model_hint,
            platform: Platform::Unknown,
            connection: if entry.is_network() {
                ConnectionType::Wireless
            } else {
                ConnectionType::Usb
            },
            state: entry.state,
            info: DeviceInfo::default(),
            capabilities: Default::default(),
            wireless_address: entry.is_network().then(|| entry.serial.clone()),
        }
    }

    /// Read properties and hardware counters for one device.
    async fn enrich(&self, entry: &AdbDeviceEntry, cancel: &CancelToken) -> CoreResult<Device> {
        if !entry.state.is_ready() {
            return Err(match entry.state {
                AdbDeviceState::Unauthorized => CoreError::Unauthorized(entry.serial.clone()),
                state => CoreError::DeviceUnavailable {
                    serial: entry.serial.clone(),
                    state: state.as_str().to_string(),
                },
            });
        }

        let props = self.client.getprops(&entry.serial, cancel).await?;
        let platform = detect_platform(&props);
        let adapter = adapter_for(platform);

        let mut info = DeviceInfo::from_props(&props);
        info.battery_percent = self.battery(&entry.serial, cancel).await;
        info.ip_address = self.device_ip(&entry.serial, cancel).await;
        if let Some((total, free)) = self.storage(&entry.serial, cancel).await {
            info.storage_total_bytes = Some(total);
            info.storage_free_bytes = Some(free);
        }

        let custom_name = self.profiles.read().await.name_for(&entry.serial);
        let model_name = adapter.display_model(&props);

        {
            let mut profiles = self.profiles.write().await;
            profiles.touch(
                &entry.serial,
                &model_name,
                entry.is_network().then_some(entry.serial.as_str()),
            );
        }

        let mut capabilities = adapter.capabilities(&props);
        if info.battery_percent.is_none() {
            capabilities.remove(Capability::BatteryInfo);
        }

        Ok(Device {
            serial: entry.serial.clone(),
            custom_name,
            model_name,
            platform,
            connection: if entry.is_network() {
                ConnectionType::Wireless
            } else {
                ConnectionType::Usb
            },
            state: entry.state,
            info,
            capabilities,
            wireless_address: entry.is_network().then(|| entry.serial.clone()),
        })
    }

    async fn battery(&self, serial: &str, cancel: &CancelToken) -> Option<u8> {
        let raw = self
            .client
            .shell(serial, &["dumpsys", "battery"], cancel)
            .await
            .ok()?;
        for line in raw.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("level:") {
                if let Ok(level) = value.trim().parse::<u8>() {
                    return Some(level.min(100));
                }
            }
        }
        None
    }

    async fn storage(&self, serial: &str, cancel: &CancelToken) -> Option<(u64, u64)> {
        // `df` output varies; ask for 1K blocks and take the data partition.
        let raw = self
            .client
            .shell(serial, &["df", "/data"], cancel)
            .await
            .ok()?;
        for line in raw.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 4 {
                let total_kb = cols[1].parse::<u64>().ok()?;
                let free_kb = cols[3].parse::<u64>().ok()?;
                return Some((total_kb * 1024, free_kb * 1024));
            }
        }
        None
    }

    async fn device_ip(&self, serial: &str, cancel: &CancelToken) -> Option<String> {
        for args in [
            vec!["ip", "route", "get", "1.1.1.1"],
            vec!["ip", "-f", "inet", "addr", "show", "wlan0"],
        ] {
            if let Ok(raw) = self.client.shell(serial, &args, cancel).await {
                if let Some(ip) = parse::parse_first_ipv4(&raw) {
                    return Some(ip);
                }
            }
        }
        None
    }

    // ---- profiles -----------------------------------------------------------

    pub async fn rename_device(&self, serial: &str, name: &str) -> CoreResult<()> {
        {
            let mut profiles = self.profiles.write().await;
            profiles.set_name(serial, name);
            profiles.save(&self.profile_path)?;
        }
        let mut cache = self.cache.write().await;
        if let Some(device) = cache.get_mut(serial) {
            device.custom_name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
        }
        Ok(())
    }

    pub async fn cached(&self, serial: &str) -> Option<Device> {
        self.cache.read().await.get(serial).cloned()
    }

    async fn require_device(&self, serial: &str) -> CoreResult<Device> {
        self.cached(serial)
            .await
            .ok_or_else(|| CoreError::DeviceNotFound(serial.to_string()))
    }

    fn adapter_for_device(device: &Device) -> Box<dyn PlatformAdapter> {
        adapter_for(device.platform)
    }

    // ---- applications -------------------------------------------------------

    /// List installed applications. `include_system` surfaces vendor packages.
    pub async fn list_apps(
        &self,
        serial: &str,
        include_system: bool,
        cancel: &CancelToken,
    ) -> CoreResult<Vec<InstalledApp>> {
        let device = self.require_device(serial).await?;
        let adapter = Self::adapter_for_device(&device);

        // `-3` = third-party only; cheaper and matches what users expect.
        let args: Vec<&str> = if include_system {
            vec!["pm", "list", "packages", "-f"]
        } else {
            vec!["pm", "list", "packages", "-3", "-f"]
        };
        let raw = self.client.shell(serial, &args, cancel).await?;

        let mut apps: Vec<InstalledApp> = parse::parse_package_list(&raw)
            .into_iter()
            .map(|(package, apk_path)| {
                let system = adapter.is_system_noise(&package);
                InstalledApp {
                    package,
                    apk_path,
                    version_name: None,
                    version_code: None,
                    system,
                }
            })
            .filter(|app| include_system || !app.system)
            .collect();

        apps.sort_by(|a, b| a.package.cmp(&b.package));
        Ok(apps)
    }

    /// Version details for one package (separate call: `dumpsys` per package
    /// is expensive, so the Library fetches it lazily).
    pub async fn app_version(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<(Option<String>, Option<u64>)> {
        let raw = self
            .client
            .shell(serial, &["dumpsys", "package", package], cancel)
            .await?;
        let mut version_name = None;
        let mut version_code = None;
        for line in raw.lines() {
            let line = line.trim();
            if version_name.is_none() {
                if let Some(v) = line.strip_prefix("versionName=") {
                    version_name = Some(v.trim().to_string());
                }
            }
            if version_code.is_none() {
                if let Some(rest) = line.strip_prefix("versionCode=") {
                    version_code = rest
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<u64>().ok());
                }
            }
            if version_name.is_some() && version_code.is_some() {
                break;
            }
        }
        Ok((version_name, version_code))
    }

    pub async fn launch_app(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<()> {
        let device = self.require_device(serial).await?;
        let adapter = Self::adapter_for_device(&device);
        if !device.capabilities.has(Capability::LaunchApp) {
            return Err(CoreError::Unsupported("launch_app"));
        }
        adapter.launch(&self.client, serial, package, cancel).await
    }

    pub async fn uninstall_app(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<()> {
        let out = self
            .client
            .exec_device(serial, &["uninstall", package], DEFAULT_TIMEOUT, cancel)
            .await?;
        let text = out.combined();
        if text.to_ascii_lowercase().contains("success") {
            Ok(())
        } else {
            Err(CoreError::AdbFailed {
                code: out.status,
                stdout: out.stdout,
                stderr: out.stderr,
            })
        }
    }

    pub async fn force_stop(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<()> {
        self.client
            .shell(serial, &["am", "force-stop", package], cancel)
            .await?;
        Ok(())
    }

    pub async fn clear_data(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<()> {
        let raw = self
            .client
            .shell(serial, &["pm", "clear", package], cancel)
            .await?;
        if raw.to_ascii_lowercase().contains("success") {
            Ok(())
        } else {
            Err(CoreError::other(raw.trim().to_string()))
        }
    }

    // ---- installation -------------------------------------------------------

    /// Install an APK, picking the fastest mechanism the device supports and
    /// falling back cleanly. `on_progress` receives raw adb progress lines.
    ///
    /// Success is not "the exit code was 0": ADB prints errors while exiting 0,
    /// and some transports print `Success` for a package the device rejected.
    /// A run is only successful when the process exited 0, an exact `Success`
    /// line was printed, no `INSTALL_FAILED_*`/`Failure` was reported, and the
    /// expected package is actually present on the device afterwards.
    pub async fn install_apk<F>(
        &self,
        serial: &str,
        apk: &ApkMetadata,
        cancel: &CancelToken,
        mut on_progress: F,
    ) -> CoreResult<InstallOutcome>
    where
        F: FnMut(&str) + Send,
    {
        let device = self.require_device(serial).await?;
        if !device.capabilities.has(Capability::InstallApk) {
            return Err(CoreError::Unsupported("install_apk"));
        }

        let path = apk.path.to_string_lossy().to_string();
        let started = std::time::Instant::now();
        let streaming = device.capabilities.has(Capability::StreamingInstall);

        // Attempt order: streaming (fast) -> standard. `-r` keeps data on
        // reinstall. Permissions are deliberately NOT granted up front: the
        // user decides that on the device.
        let attempts: Vec<(&'static str, Vec<&str>)> = if streaming {
            vec![
                ("streaming", vec!["install", "-r", "--streaming", &path]),
                ("standard", vec!["install", "-r", &path]),
            ]
        } else {
            vec![("standard", vec!["install", "-r", &path])]
        };

        let mut last_error = None;
        for (method, args) in attempts {
            if cancel.is_cancelled() {
                return Err(CoreError::Cancelled);
            }

            let mut full: Vec<&str> = vec!["-s", serial];
            full.extend(args.iter().copied());

            let result = self
                .client
                .exec_streaming(&full, LONG_TIMEOUT, cancel, &mut on_progress)
                .await;

            let out = match result {
                Ok(out) => out,
                Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                Err(e) => {
                    last_error = Some(e);
                    tracing::warn!(serial, method, "install attempt failed, trying fallback");
                    continue;
                }
            };

            let text = out.combined();

            // A real device rejection outranks any "Success" in the same output.
            if let Some(code) = parse::parse_install_failure(&text) {
                return Err(CoreError::InstallFailed {
                    code,
                    raw: text.trim().to_string(),
                });
            }

            let reported_success =
                out.status == Some(0) && text.lines().any(|line| line.trim() == "Success");
            if !reported_success {
                last_error = Some(CoreError::AdbFailed {
                    code: out.status,
                    stdout: out.stdout,
                    stderr: out.stderr,
                });
                tracing::warn!(
                    serial,
                    method,
                    "install did not report success, trying fallback"
                );
                continue;
            }

            // The transport may report success for a package the device did
            // not actually keep; verify before telling the user it worked.
            self.verify_installed(serial, &apk.package, cancel).await?;

            return Ok(InstallOutcome {
                package: Some(apk.package.clone()),
                serial: serial.to_string(),
                method,
                duration_ms: started.elapsed().as_millis(),
            });
        }

        Err(last_error.unwrap_or_else(|| CoreError::other("installation failed")))
    }

    /// Confirm the expected package is present after a reported install success.
    async fn verify_installed(
        &self,
        serial: &str,
        package: &str,
        cancel: &CancelToken,
    ) -> CoreResult<()> {
        let raw = self
            .client
            .shell(serial, &["pm", "path", package], cancel)
            .await?;
        if raw
            .lines()
            .any(|line| line.trim_start().starts_with("package:"))
        {
            Ok(())
        } else {
            Err(CoreError::InstallNotVerified {
                package: package.to_string(),
            })
        }
    }

    /// Install one APK onto several devices, reporting each result separately.
    pub async fn install_on_devices(
        &self,
        serials: &[String],
        apk: &ApkMetadata,
        cancel: &CancelToken,
    ) -> Vec<(String, CoreResult<InstallOutcome>)> {
        let mut results = Vec::with_capacity(serials.len());
        for serial in serials {
            if cancel.is_cancelled() {
                results.push((serial.clone(), Err(CoreError::Cancelled)));
                continue;
            }
            let outcome = self.install_apk(serial, apk, cancel, |_| {}).await;
            results.push((serial.clone(), outcome));
        }
        results
    }

    // ---- wireless -----------------------------------------------------------

    /// Move a USB-connected device onto Wi-Fi ADB without QR pairing.
    pub async fn enable_wireless(&self, serial: &str, cancel: &CancelToken) -> CoreResult<String> {
        let device = self.require_device(serial).await?;
        if device.connection == ConnectionType::Wireless {
            return Ok(device
                .wireless_address
                .unwrap_or_else(|| serial.to_string()));
        }
        if !device.capabilities.has(Capability::WirelessAdb) {
            return Err(CoreError::Unsupported("wireless_adb"));
        }

        let ip = device
            .info
            .ip_address
            .clone()
            .or(self.device_ip(serial, cancel).await)
            .ok_or_else(|| {
                CoreError::Wireless(
                    "the headset is not on a Wi-Fi network, so it has no address to connect to"
                        .into(),
                )
            })?;

        const PORT: u16 = 5555;
        self.client.tcpip(serial, PORT, cancel).await?;
        // The daemon needs a moment to rebind before it accepts connections.
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

        let addr = format!("{ip}:{PORT}");
        self.client.connect(&addr, cancel).await?;

        // Verify: the address must now appear as a ready device.
        let entries = self.client.list_devices(cancel).await?;
        let ok = entries
            .iter()
            .any(|e| e.serial == addr && e.state.is_ready());
        if !ok {
            return Err(CoreError::Wireless(format!(
                "connected to {addr} but the headset did not come online"
            )));
        }

        {
            let mut profiles = self.profiles.write().await;
            profiles.touch(serial, &device.model_name, Some(&addr));
            let _ = profiles.save(&self.profile_path);
        }

        Ok(addr)
    }

    pub async fn connect_wireless(&self, addr: &str, cancel: &CancelToken) -> CoreResult<String> {
        let addr = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:5555")
        };
        self.client.connect(&addr, cancel).await?;
        Ok(addr)
    }

    pub async fn disconnect_wireless(&self, addr: &str, cancel: &CancelToken) -> CoreResult<()> {
        self.client.disconnect(addr, cancel).await
    }

    /// Pre-flight check used by the install screen: does this APK fit this
    /// device? Returns a list of human-readable blockers (empty = good).
    pub async fn check_compatibility(&self, serial: &str, apk: &ApkMetadata) -> Vec<String> {
        let Some(device) = self.cached(serial).await else {
            return vec!["That device is no longer connected.".to_string()];
        };
        let mut problems = Vec::new();

        if !device.info.supported_abis.is_empty()
            && !apk.abi_compatible_with(&device.info.supported_abis)
        {
            problems.push(format!(
                "This app is built for {} and {} uses {}.",
                apk.abis.join(", "),
                device.display_name(),
                device.info.supported_abis.join(", ")
            ));
        }

        if let (Some(min_sdk), Some(device_sdk)) = (apk.min_sdk, device.info.sdk_version) {
            if min_sdk > device_sdk {
                problems.push(format!(
                    "This app needs a newer system version than {} has.",
                    device.display_name()
                ));
            }
        }

        if let Some(free) = device.info.storage_free_bytes {
            // Installation needs roughly twice the APK size transiently.
            if free < apk.size_bytes.saturating_mul(2) {
                problems.push(format!(
                    "{} may not have enough free space for this app.",
                    device.display_name()
                ));
            }
        }

        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> DeviceManager {
        // Every test needs its OWN file: they run in parallel threads of one
        // process, so a PID-based name is shared and one test's cleanup would
        // delete another test's profiles mid-run.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut profile_path = std::env::temp_dir();
        profile_path.push(format!("vrmanager-dm-{}-{}.json", std::process::id(), n));
        let _ = std::fs::remove_file(&profile_path);
        DeviceManager::new(AdbClient::new("Z:/no/such/adb.exe"), profile_path)
    }

    pub(super) fn test_device(serial: &str, abis: &[&str], sdk: u32, free: u64) -> Device {
        Device {
            serial: serial.into(),
            custom_name: Some("My Pico".into()),
            model_name: "Pico 4".into(),
            platform: Platform::Pico,
            connection: ConnectionType::Usb,
            state: AdbDeviceState::Device,
            info: DeviceInfo {
                supported_abis: abis.iter().map(|s| s.to_string()).collect(),
                sdk_version: Some(sdk),
                storage_free_bytes: Some(free),
                ..Default::default()
            },
            capabilities: crate::core::platform::capability::Capabilities::baseline_android(),
            wireless_address: None,
        }
    }

    fn test_apk(abis: &[&str], min_sdk: u32, size: u64) -> ApkMetadata {
        ApkMetadata {
            path: PathBuf::from("a.apk"),
            file_name: "a.apk".into(),
            size_bytes: size,
            package: "com.test.app".into(),
            application_label: Some("Test".into()),
            version_name: Some("1.0".into()),
            version_code: Some(1),
            min_sdk: Some(min_sdk),
            target_sdk: Some(32),
            abis: abis.iter().map(|s| s.to_string()).collect(),
            permissions: vec![],
            has_icon: true,
            vr_capable: true,
        }
    }

    #[tokio::test]
    async fn operations_on_unknown_device_fail_cleanly() {
        let dm = manager();
        let cancel = CancelToken::new();
        let err = dm.launch_app("NOPE", "com.x", &cancel).await.unwrap_err();
        assert!(matches!(err, CoreError::DeviceNotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn renaming_persists_and_updates_cache() {
        let dm = manager();
        dm.cache
            .write()
            .await
            .insert("S1".into(), test_device("S1", &["arm64-v8a"], 32, 1 << 30));

        dm.rename_device("S1", "Brother's Pico").await.unwrap();
        assert_eq!(
            dm.cached("S1").await.unwrap().display_name(),
            "Brother's Pico"
        );

        let reloaded = ProfileStore::load(&dm.profile_path).unwrap();
        assert_eq!(reloaded.name_for("S1").unwrap(), "Brother's Pico");
        let _ = std::fs::remove_file(&dm.profile_path);
    }

    #[tokio::test]
    async fn compatibility_flags_abi_sdk_and_storage() {
        let dm = manager();
        dm.cache.write().await.insert(
            "S1".into(),
            test_device("S1", &["arm64-v8a"], 29, 50 * 1024 * 1024),
        );

        // x86 app on an arm64 headset, needs SDK 33, 100MB on a 50MB device.
        let bad = test_apk(&["x86_64"], 33, 100 * 1024 * 1024);
        let problems = dm.check_compatibility("S1", &bad).await;
        assert_eq!(problems.len(), 3, "got {problems:#?}");
        assert!(problems.iter().any(|p| p.contains("x86_64")));
        assert!(problems.iter().any(|p| p.contains("newer system version")));
        assert!(problems.iter().any(|p| p.contains("free space")));

        // A well-matched app has no blockers.
        let good = test_apk(&["arm64-v8a"], 29, 1024 * 1024);
        assert!(dm.check_compatibility("S1", &good).await.is_empty());
    }

    #[tokio::test]
    async fn compatibility_on_missing_device_is_explicit() {
        let dm = manager();
        let problems = dm.check_compatibility("GONE", &test_apk(&[], 29, 1)).await;
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("no longer connected"));
    }

    #[tokio::test]
    async fn install_requires_capability() {
        let dm = manager();
        let mut device = test_device("S1", &["arm64-v8a"], 32, 1 << 30);
        device.capabilities.remove(Capability::InstallApk);
        dm.cache.write().await.insert("S1".into(), device);

        let err = dm
            .install_apk("S1", &test_apk(&[], 29, 1), &CancelToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(
            matches!(err, CoreError::Unsupported("install_apk")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn wireless_requires_an_ip() {
        let dm = manager();
        // Device with no IP and a dead adb binary: must fail with a wireless
        // error explaining the cause, not a raw adb error.
        dm.cache
            .write()
            .await
            .insert("S1".into(), test_device("S1", &["arm64-v8a"], 32, 1 << 30));
        let err = dm
            .enable_wireless("S1", &CancelToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Wireless(_)), "got {err:?}");
    }
}
