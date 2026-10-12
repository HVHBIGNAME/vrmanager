//! Bounded IPC selections. Validate identifiers; never silently repair them.
use std::collections::HashSet;
use std::path::Path;

use crate::error::{UiError, UiResult};

pub const MAX_APKS: usize = 64;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_SERIALS: usize = 32;
pub const MAX_SERIAL_BYTES: usize = 256;

pub fn validate_serial(serial: &str) -> UiResult<()> {
    if serial.is_empty()
        || serial.len() > MAX_SERIAL_BYTES
        || serial.starts_with('-')
        || !serial
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-[]".contains(&b))
    {
        return Err(UiError::new(
            "invalidSerial",
            "Choose a valid device identifier.",
        ));
    }
    Ok(())
}

pub(super) fn validate_items(items: &[super::QueuedInstall]) -> UiResult<()> {
    let mut ids = HashSet::new();
    for item in items {
        validate_serial(&item.serial)?;
        let path = item
            .apk_path
            .to_str()
            .ok_or_else(|| UiError::new("invalidApkPath", "The APK path is not valid text."))?;
        apk_paths(vec![path.to_owned()])?;
        if item.item_id.is_empty() || item.item_id.len() > 128 || !ids.insert(&item.item_id) {
            return Err(UiError::new(
                "invalidInstallItem",
                "Installation items must have distinct identifiers.",
            ));
        }
    }
    Ok(())
}

pub fn apk_paths(paths: Vec<String>) -> UiResult<Vec<String>> {
    if paths.is_empty() {
        return Err(UiError::new(
            "noApkSelected",
            "Choose at least one app to install.",
        ));
    }
    if paths.len() > MAX_APKS {
        return Err(UiError::new(
            "selectionTooLarge",
            "Choose at most 64 APK files at a time.",
        ));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        if path.len() > MAX_PATH_BYTES
            || path.chars().any(char::is_control)
            || !Path::new(&path).is_absolute()
        {
            return Err(UiError::new(
                "invalidApkPath",
                "Choose an APK using its full local file path.",
            ));
        }
        if seen.insert(path.clone()) {
            result.push(path);
        }
    }
    Ok(result)
}
