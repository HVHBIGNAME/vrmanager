//! Device capability model.
//!
//! The UI is built from capabilities, never from `if platform == Pico`.
//! An adapter declares what a concrete headset can do; screens enable,
//! disable or hide affordances based on the answer.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    InstallApk,
    UninstallApk,
    LaunchApp,
    ForceStop,
    ClearData,
    ClearCache,
    ExportApk,
    ListApps,
    WirelessAdb,
    StreamingInstall,
    ScreenCapture,
    BatteryInfo,
    StorageInfo,
    TemperatureInfo,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstallApk => "install_apk",
            Self::UninstallApk => "uninstall_apk",
            Self::LaunchApp => "launch_app",
            Self::ForceStop => "force_stop",
            Self::ClearData => "clear_data",
            Self::ClearCache => "clear_cache",
            Self::ExportApk => "export_apk",
            Self::ListApps => "list_apps",
            Self::WirelessAdb => "wireless_adb",
            Self::StreamingInstall => "streaming_install",
            Self::ScreenCapture => "screen_capture",
            Self::BatteryInfo => "battery_info",
            Self::StorageInfo => "storage_info",
            Self::TemperatureInfo => "temperature_info",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capabilities(BTreeSet<Capability>);

impl Capabilities {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    pub fn with(mut self, cap: Capability) -> Self {
        self.0.insert(cap);
        self
    }

    pub fn insert(&mut self, cap: Capability) {
        self.0.insert(cap);
    }

    pub fn remove(&mut self, cap: Capability) {
        self.0.remove(&cap);
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Everything a generic, healthy Android-based headset can do over ADB.
    pub fn baseline_android() -> Self {
        Self::new()
            .with(Capability::InstallApk)
            .with(Capability::UninstallApk)
            .with(Capability::LaunchApp)
            .with(Capability::ForceStop)
            .with(Capability::ClearData)
            .with(Capability::ClearCache)
            .with(Capability::ExportApk)
            .with(Capability::ListApps)
            .with(Capability::WirelessAdb)
            .with(Capability::ScreenCapture)
            .with(Capability::BatteryInfo)
            .with(Capability::StorageInfo)
    }
}

impl FromIterator<Capability> for Capabilities {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_is_sane_and_serializes_as_a_list() {
        let caps = Capabilities::baseline_android();
        assert!(caps.has(Capability::InstallApk));
        assert!(caps.has(Capability::ListApps));
        // streaming install is a per-device discovery, never assumed
        assert!(!caps.has(Capability::StreamingInstall));

        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.starts_with('['), "got {json}");
        assert!(json.contains("installApk"));
    }

    #[test]
    fn capabilities_round_trip() {
        let caps = Capabilities::new()
            .with(Capability::LaunchApp)
            .with(Capability::StreamingInstall);
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert!(back.has(Capability::StreamingInstall));
        assert_eq!(back.len(), 2);
    }
}
