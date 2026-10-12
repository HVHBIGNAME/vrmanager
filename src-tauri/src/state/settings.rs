//! Application settings, persisted next to the device profiles.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Explicit ADB path chosen by the user; `None` = auto-detect.
    pub adb_path: Option<PathBuf>,
    /// Backend base URL; `None` = offline mode (local device management only).
    pub backend_url: Option<String>,
    /// Show vendor/system packages in the Library.
    pub show_system_apps: bool,
    /// Serials the user has selected in the global device selector.
    pub selected_serials: Vec<String>,
    /// Completed the first-run flow?
    pub onboarded: bool,
}

impl Settings {
    pub fn default_path() -> PathBuf {
        let base = dirs::config_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(std::env::temp_dir);
        base.join("VRManager").join("settings.json")
    }

    pub fn load(path: &Path) -> CoreResult<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) if raw.trim().is_empty() => Ok(Self::default()),
            Ok(raw) => Ok(serde_json::from_str(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_file_yields_defaults() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "vrmanager-settings-missing-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let settings = Settings::load(&path).unwrap();
        assert!(settings.adb_path.is_none());
        assert!(!settings.onboarded);
        assert!(settings.selected_serials.is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let mut path = std::env::temp_dir();
        path.push(format!("vrmanager-settings-rt-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let settings = Settings {
            adb_path: Some(PathBuf::from("C:/sdk/adb.exe")),
            backend_url: None,
            show_system_apps: true,
            selected_serials: vec!["S1".into(), "S2".into()],
            onboarded: true,
        };
        settings.save(&path).unwrap();

        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.adb_path, Some(PathBuf::from("C:/sdk/adb.exe")));
        assert!(loaded.show_system_apps);
        assert_eq!(loaded.selected_serials.len(), 2);
        assert!(loaded.onboarded);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_fields() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "vrmanager-settings-partial-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"showSystemApps":true}"#).unwrap();

        let loaded = Settings::load(&path).unwrap();
        assert!(loaded.show_system_apps);
        assert!(!loaded.onboarded);

        let _ = std::fs::remove_file(&path);
    }
}
