//! Per-device user profiles, persisted locally.
//!
//! Two identical Pico 4 units must stay distinguishable, so the profile is
//! keyed by ADB serial while the user only ever sees the name they chose.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    pub serial: String,
    pub custom_name: String,
    #[serde(default)]
    pub last_model: String,
    #[serde(default)]
    pub last_wireless_address: Option<String>,
    #[serde(default)]
    pub first_seen_unix: u64,
    #[serde(default)]
    pub last_seen_unix: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// JSON-backed profile store. Small enough that a single file beats a DB.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default)]
    profiles: HashMap<String, DeviceProfile>,
}

impl ProfileStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Default location: the per-user app data dir.
    pub fn default_path() -> PathBuf {
        let base = dirs::data_dir()
            .or_else(dirs::config_dir)
            .unwrap_or_else(std::env::temp_dir);
        base.join("VRManager").join("devices.json")
    }

    pub fn load(path: &Path) -> CoreResult<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) if raw.trim().is_empty() => Ok(Self::new()),
            Ok(raw) => Ok(serde_json::from_str(&raw)?),
            // A missing file is the normal first-run case, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        // Write-then-rename so a crash cannot truncate the user's names.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn get(&self, serial: &str) -> Option<&DeviceProfile> {
        self.profiles.get(serial)
    }

    pub fn name_for(&self, serial: &str) -> Option<String> {
        self.profiles
            .get(serial)
            .map(|p| p.custom_name.clone())
            .filter(|n| !n.trim().is_empty())
    }

    pub fn set_name(&mut self, serial: &str, name: &str) -> &DeviceProfile {
        let now = now_unix();
        let entry = self
            .profiles
            .entry(serial.to_string())
            .or_insert_with(|| DeviceProfile {
                serial: serial.to_string(),
                custom_name: String::new(),
                last_model: String::new(),
                last_wireless_address: None,
                first_seen_unix: now,
                last_seen_unix: now,
            });
        entry.custom_name = name.trim().to_string();
        entry.last_seen_unix = now;
        entry
    }

    /// Record a sighting; keeps `last_model`/address fresh for offline display.
    pub fn touch(&mut self, serial: &str, model: &str, wireless: Option<&str>) {
        let now = now_unix();
        let entry = self
            .profiles
            .entry(serial.to_string())
            .or_insert_with(|| DeviceProfile {
                serial: serial.to_string(),
                custom_name: String::new(),
                last_model: model.to_string(),
                last_wireless_address: wireless.map(str::to_string),
                first_seen_unix: now,
                last_seen_unix: now,
            });
        if !model.is_empty() {
            entry.last_model = model.to_string();
        }
        if let Some(addr) = wireless {
            entry.last_wireless_address = Some(addr.to_string());
        }
        entry.last_seen_unix = now;
    }

    pub fn all(&self) -> impl Iterator<Item = &DeviceProfile> {
        self.profiles.values()
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "vrmanager-test-{}-{}.json",
            name,
            std::process::id()
        ));
        p
    }

    #[test]
    fn identical_models_keep_separate_names() {
        let mut store = ProfileStore::new();
        store.set_name("SERIAL_A", "My Pico");
        store.set_name("SERIAL_B", "Test Pico");
        store.touch("SERIAL_A", "Pico 4", None);
        store.touch("SERIAL_B", "Pico 4", None);

        assert_eq!(store.name_for("SERIAL_A").unwrap(), "My Pico");
        assert_eq!(store.name_for("SERIAL_B").unwrap(), "Test Pico");
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn blank_name_is_treated_as_no_name() {
        let mut store = ProfileStore::new();
        store.set_name("S", "   ");
        assert!(store.name_for("S").is_none());
    }

    #[test]
    fn missing_file_loads_as_empty_store() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let store = ProfileStore::load(&path).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);

        let mut store = ProfileStore::new();
        store.set_name("SERIAL_A", "My Pico");
        store.touch("SERIAL_A", "Pico 4", Some("192.168.1.44:5555"));
        store.save(&path).unwrap();

        let loaded = ProfileStore::load(&path).unwrap();
        let profile = loaded.get("SERIAL_A").unwrap();
        assert_eq!(profile.custom_name, "My Pico");
        assert_eq!(profile.last_model, "Pico 4");
        assert_eq!(
            profile.last_wireless_address.as_deref(),
            Some("192.168.1.44:5555")
        );
        assert!(profile.first_seen_unix > 0);

        let _ = std::fs::remove_file(&path);
    }
}
