//! Device model.
//!
//! Product rule encoded here: the serial is an internal identifier. Anything
//! the user sees leads with their own name for the device, then the model.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::adb::parse::AdbDeviceState;
use crate::core::platform::capability::Capabilities;
use crate::core::platform::detect::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionType {
    Usb,
    Wireless,
}

impl ConnectionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Wireless => "Wi-Fi",
        }
    }
}

/// Hardware/OS facts read from the device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub device_codename: String,
    pub android_version: String,
    pub sdk_version: Option<u32>,
    pub build_fingerprint: String,
    pub abi: String,
    pub supported_abis: Vec<String>,
    pub battery_percent: Option<u8>,
    pub battery_charging: Option<bool>,
    pub temperature_celsius: Option<f32>,
    pub storage_total_bytes: Option<u64>,
    pub storage_free_bytes: Option<u64>,
    pub ram_total_bytes: Option<u64>,
    pub ip_address: Option<String>,
}

impl DeviceInfo {
    /// Build from a `getprop` dump; hardware counters are filled separately.
    pub fn from_props(props: &HashMap<String, String>) -> Self {
        let get = |k: &str| props.get(k).cloned().unwrap_or_default();

        let supported_abis = {
            let raw = get("ro.product.cpu.abilist");
            if raw.is_empty() {
                let single = get("ro.product.cpu.abi");
                if single.is_empty() {
                    Vec::new()
                } else {
                    vec![single]
                }
            } else {
                raw.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
        };

        Self {
            manufacturer: get("ro.product.manufacturer"),
            model: get("ro.product.model"),
            device_codename: get("ro.product.device"),
            android_version: get("ro.build.version.release"),
            sdk_version: get("ro.build.version.sdk").parse().ok(),
            build_fingerprint: get("ro.build.fingerprint"),
            abi: supported_abis.first().cloned().unwrap_or_default(),
            supported_abis,
            ..Default::default()
        }
    }

    pub fn storage_free_gb(&self) -> Option<f64> {
        self.storage_free_bytes
            .map(|b| b as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

/// A connected device as the core knows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Internal identity (ADB serial or `host:port`). Not a headline value.
    pub serial: String,
    /// User-assigned name, when the profile has one.
    pub custom_name: Option<String>,
    /// Marketing name resolved by the platform adapter ("Pico 4").
    pub model_name: String,
    pub platform: Platform,
    pub connection: ConnectionType,
    pub state: AdbDeviceState,
    pub info: DeviceInfo,
    pub capabilities: Capabilities,
    /// Set when the device is reachable over the network.
    pub wireless_address: Option<String>,
}

impl Device {
    /// What the UI prints as the device's primary label.
    pub fn display_name(&self) -> &str {
        match self.custom_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => &self.model_name,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state.is_ready()
    }
}

/// Lightweight projection used by lists and the global device selector.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub serial: String,
    pub display_name: String,
    pub model_name: String,
    pub platform: Platform,
    pub connection: ConnectionType,
    pub state: AdbDeviceState,
    pub ready: bool,
    pub battery_percent: Option<u8>,
    pub storage_free_bytes: Option<u64>,
}

impl From<&Device> for DeviceSnapshot {
    fn from(d: &Device) -> Self {
        Self {
            serial: d.serial.clone(),
            display_name: d.display_name().to_string(),
            model_name: d.model_name.clone(),
            platform: d.platform,
            connection: d.connection,
            state: d.state,
            ready: d.is_ready(),
            battery_percent: d.info.battery_percent,
            storage_free_bytes: d.info.storage_free_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_abi_list_and_sdk() {
        let info = DeviceInfo::from_props(&props(&[
            ("ro.product.cpu.abilist", "arm64-v8a,armeabi-v7a,armeabi"),
            ("ro.build.version.sdk", "32"),
            ("ro.build.version.release", "12"),
        ]));
        assert_eq!(info.abi, "arm64-v8a");
        assert_eq!(info.supported_abis.len(), 3);
        assert_eq!(info.sdk_version, Some(32));
        assert_eq!(info.android_version, "12");
    }

    #[test]
    fn falls_back_to_single_abi_property() {
        let info = DeviceInfo::from_props(&props(&[("ro.product.cpu.abi", "arm64-v8a")]));
        assert_eq!(info.supported_abis, vec!["arm64-v8a"]);
    }

    #[test]
    fn custom_name_wins_over_model_but_blank_does_not() {
        let mut device = Device {
            serial: "PA7B10K2".into(),
            custom_name: Some("My Pico".into()),
            model_name: "Pico 4".into(),
            platform: Platform::Pico,
            connection: ConnectionType::Usb,
            state: AdbDeviceState::Device,
            info: DeviceInfo::default(),
            capabilities: Capabilities::baseline_android(),
            wireless_address: None,
        };
        assert_eq!(device.display_name(), "My Pico");

        device.custom_name = Some("   ".into());
        assert_eq!(device.display_name(), "Pico 4");

        device.custom_name = None;
        assert_eq!(device.display_name(), "Pico 4");

        // The snapshot never leads with a serial.
        let snap = DeviceSnapshot::from(&device);
        assert_eq!(snap.display_name, "Pico 4");
        assert!(snap.ready);
    }
}
