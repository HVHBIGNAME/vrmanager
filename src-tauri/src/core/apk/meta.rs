//! APK metadata extraction: open the ZIP, decode `AndroidManifest.xml`,
//! report what the install screen needs before anything touches a device.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::apk::axml;
use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkMetadata {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub package: String,
    /// Human label when the manifest stores a literal; `None` when it is a
    /// resource reference we deliberately do not resolve.
    pub application_label: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<u64>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    /// ABIs inferred from `lib/<abi>/` entries; empty means architecture-agnostic.
    pub abis: Vec<String>,
    pub permissions: Vec<String>,
    pub has_icon: bool,
    /// True when the manifest declares VR/XR intent filters or features.
    pub vr_capable: bool,
}

impl ApkMetadata {
    /// Display name for the UI: label if present, else the last package segment.
    pub fn display_name(&self) -> String {
        if let Some(label) = self
            .application_label
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('@'))
        {
            return label.to_string();
        }
        self.package
            .rsplit('.')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => s.to_string(),
                }
            })
            .unwrap_or_else(|| self.package.clone())
    }

    pub fn size_mb(&self) -> f64 {
        self.size_bytes as f64 / 1024.0 / 1024.0
    }

    /// Will this APK run on a device advertising `device_abis`?
    pub fn abi_compatible_with(&self, device_abis: &[String]) -> bool {
        if self.abis.is_empty() {
            return true; // pure-Java / universal
        }
        self.abis
            .iter()
            .any(|a| device_abis.iter().any(|d| d.eq_ignore_ascii_case(a)))
    }
}

/// Inspect an APK file on disk.
pub fn inspect_apk(path: &Path) -> CoreResult<ApkMetadata> {
    let file = File::open(path)
        .map_err(|e| CoreError::Apk(format!("cannot open {}: {e}", path.display())))?;
    let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);

    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| CoreError::Apk(format!("not a valid APK archive: {e}")))?;

    // --- manifest ---
    let manifest_bytes = {
        let entry = zip
            .by_name("AndroidManifest.xml")
            .map_err(|_| CoreError::Apk("APK has no AndroidManifest.xml".into()))?;
        let declared_size = entry.size();
        read_manifest(entry, declared_size)?
    };

    let doc = axml::parse(&manifest_bytes)
        .ok_or_else(|| CoreError::Apk("AndroidManifest.xml is not valid binary XML".into()))?;

    let manifest = doc
        .find("manifest")
        .ok_or_else(|| CoreError::Apk("manifest element missing".into()))?;

    let package = manifest
        .attr("package")
        .filter(|package| valid_package_name(package))
        .map(str::to_string)
        .ok_or_else(|| CoreError::Apk("manifest has no valid Android package name".into()))?;

    let version_name = manifest
        .attr("versionName")
        .map(str::to_string)
        .filter(|v| !v.is_empty() && !v.starts_with('@'));
    let version_code = manifest
        .attr("versionCode")
        .and_then(|v| v.parse::<u64>().ok());

    let uses_sdk = doc.find("uses-sdk");
    let min_sdk = uses_sdk
        .and_then(|e| e.attr("minSdkVersion"))
        .and_then(|v| v.parse::<u32>().ok());
    let target_sdk = uses_sdk
        .and_then(|e| e.attr("targetSdkVersion"))
        .and_then(|v| v.parse::<u32>().ok());

    let application_label = doc
        .find("application")
        .and_then(|e| e.attr("label"))
        .map(str::to_string)
        .filter(|l| !l.is_empty());

    let permissions: Vec<String> = doc
        .find_all("uses-permission")
        .filter_map(|e| e.attr("name"))
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let features: Vec<String> = doc
        .find_all("uses-feature")
        .filter_map(|e| e.attr("name"))
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let categories: Vec<String> = doc
        .find_all("category")
        .filter_map(|e| e.attr("name"))
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let vr_capable = features.iter().any(|f| {
        f.contains("vr.headtracking")
            || f.contains("oculus")
            || f.contains("pvr")
            || f.contains("xr")
    }) || categories
        .iter()
        .any(|c| c.contains("cardboard") || c.contains("daydream") || c.contains("vr"));

    // --- archive-derived facts ---
    let mut abis: BTreeSet<String> = BTreeSet::new();
    let mut has_icon = false;
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let name = entry.name();
        if let Some(rest) = name.strip_prefix("lib/") {
            if let Some((abi, _)) = rest.split_once('/') {
                if !abi.is_empty() {
                    abis.insert(abi.to_string());
                }
            }
        }
        if !has_icon
            && (name.starts_with("res/") || name.starts_with("resources"))
            && name.to_ascii_lowercase().contains("icon")
        {
            has_icon = true;
        }
    }

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| package.clone());

    Ok(ApkMetadata {
        path: path.to_path_buf(),
        file_name,
        size_bytes,
        package,
        application_label,
        version_name,
        version_code,
        min_sdk,
        target_sdk,
        abis: abis.into_iter().collect(),
        permissions,
        has_icon,
        vr_capable,
    })
}

/// Match the launch boundary: ASCII identifier components, no trimming or
/// case folding. Single-component framework packages such as `android` work.
fn valid_package_name(package: &str) -> bool {
    package.len() <= 255
        && package.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// Check the ZIP hint before allocating, then distrust it while decompressing.
fn read_manifest(source: impl Read, declared_size: u64) -> CoreResult<Vec<u8>> {
    let limit = super::MAX_MANIFEST_BYTES;
    let too_large = || CoreError::Apk("manifest exceeds the 8 MiB inspection limit".into());
    if declared_size > limit as u64 {
        return Err(too_large());
    }
    // A single sentinel byte detects an understated ZIP size. Stack reads keep
    // it out of the heap buffer; even the final allocation stays within limit.
    let mut source = source.take(limit as u64 + 1);
    let mut buf = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let count = match source.read(&mut block) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(CoreError::Apk(format!("cannot read manifest: {error}"))),
        };
        if count == 0 {
            return Ok(buf);
        }
        let len = buf.len().checked_add(count).ok_or_else(too_large)?;
        if len > limit {
            return Err(too_large());
        }
        if len > buf.capacity() {
            let capacity = buf.capacity().max(block.len()).saturating_mul(2).min(limit);
            buf.try_reserve_exact(capacity.max(len) - buf.len())
                .map_err(|_| CoreError::Apk("cannot allocate manifest buffer".into()))?;
        }
        buf.extend_from_slice(&block[..count]);
    }
}

#[cfg(test)]
#[path = "meta_boundary_tests.rs"]
mod boundary_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(package: &str, label: Option<&str>, abis: &[&str]) -> ApkMetadata {
        ApkMetadata {
            path: PathBuf::from("x.apk"),
            file_name: "x.apk".into(),
            size_bytes: 130_023_424,
            package: package.into(),
            application_label: label.map(str::to_string),
            version_name: Some("1.4.2".into()),
            version_code: Some(10402),
            min_sdk: Some(29),
            target_sdk: Some(32),
            abis: abis.iter().map(|s| s.to_string()).collect(),
            permissions: vec![],
            has_icon: true,
            vr_capable: true,
        }
    }

    #[test]
    fn display_name_prefers_label() {
        assert_eq!(
            meta("com.vrchat.vrchat", Some("VRChat"), &[]).display_name(),
            "VRChat"
        );
    }

    #[test]
    fn resource_reference_label_is_ignored() {
        let m = meta("com.beatgames.beatsaber", Some("@0x7f110001"), &[]);
        assert_eq!(m.display_name(), "Beatsaber");
    }

    #[test]
    fn missing_label_falls_back_to_package_tail() {
        assert_eq!(
            meta("com.example.telegram", None, &[]).display_name(),
            "Telegram"
        );
    }

    #[test]
    fn abi_compatibility_rules() {
        let arm64 = meta("a.b", None, &["arm64-v8a"]);
        assert!(arm64.abi_compatible_with(&["arm64-v8a".into(), "armeabi-v7a".into()]));
        assert!(!arm64.abi_compatible_with(&["x86_64".into()]));

        let universal = meta("a.b", None, &[]);
        assert!(universal.abi_compatible_with(&["x86_64".into()]));
        assert!(universal.abi_compatible_with(&[]));
    }

    #[test]
    fn size_is_reported_in_mb() {
        assert!((meta("a.b", None, &[]).size_mb() - 124.0).abs() < 0.5);
    }

    #[test]
    fn inspecting_a_non_apk_is_a_clean_error() {
        let mut path = std::env::temp_dir();
        path.push(format!("vrmanager-not-an-apk-{}.apk", std::process::id()));
        std::fs::write(&path, b"definitely not a zip").unwrap();

        let err = inspect_apk(&path).unwrap_err();
        assert!(matches!(err, CoreError::Apk(_)), "got {err:?}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inspecting_a_missing_file_is_a_clean_error() {
        let err = inspect_apk(Path::new("Z:/nope/missing.apk")).unwrap_err();
        assert!(matches!(err, CoreError::Apk(_)));
    }
}
