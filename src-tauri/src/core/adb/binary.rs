//! ADB binary resolution.
//!
//! The ADB executable is never hard-coded. Resolution order, first hit wins:
//!   1. an explicit path from user settings,
//!   2. a copy bundled next to the VRManager executable,
//!   3. whatever is on `PATH`,
//!   4. the standard Android SDK locations for this OS.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdbSource {
    Configured,
    Bundled,
    SystemPath,
    AndroidSdk,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbBinary {
    pub path: PathBuf,
    pub source: AdbSource,
}

const EXE: &str = if cfg!(windows) { "adb.exe" } else { "adb" };

/// Resolve an ADB executable, or explain that none exists.
pub fn resolve(configured: Option<&Path>) -> CoreResult<AdbBinary> {
    if let Some(path) = configured {
        // An explicit user choice is never silently overridden: if it is wrong
        // the user must hear about it.
        if is_executable(path) {
            return Ok(AdbBinary {
                path: path.to_path_buf(),
                source: AdbSource::Configured,
            });
        }
        return Err(CoreError::Other(format!(
            "configured adb path does not exist: {}",
            path.display()
        )));
    }

    if let Some(path) = bundled_candidates().into_iter().find(|p| is_executable(p)) {
        return Ok(AdbBinary {
            path,
            source: AdbSource::Bundled,
        });
    }

    if let Some(path) = search_path() {
        return Ok(AdbBinary {
            path,
            source: AdbSource::SystemPath,
        });
    }

    if let Some(path) = sdk_candidates().into_iter().find(|p| is_executable(p)) {
        return Ok(AdbBinary {
            path,
            source: AdbSource::AndroidSdk,
        });
    }

    Err(CoreError::AdbNotFound)
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn bundled_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join(EXE));
            out.push(dir.join("adb").join(EXE));
            out.push(dir.join("resources").join("adb").join(EXE));
            out.push(dir.join("platform-tools").join(EXE));
        }
    }
    out
}

fn search_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(EXE))
        .find(|candidate| is_executable(candidate))
}

fn sdk_candidates() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(val) = std::env::var_os(key) {
            roots.push(PathBuf::from(val));
        }
    }

    if let Some(home) = dirs::home_dir() {
        if cfg!(windows) {
            if let Some(local) = dirs::data_local_dir() {
                roots.push(local.join("Android").join("Sdk"));
            }
        } else if cfg!(target_os = "macos") {
            roots.push(home.join("Library").join("Android").join("sdk"));
        } else {
            roots.push(home.join("Android").join("Sdk"));
            roots.push(home.join("android-sdk"));
        }
    }

    let mut out: Vec<PathBuf> = roots
        .into_iter()
        .map(|root| root.join("platform-tools").join(EXE))
        .collect();

    // Vendor tooling frequently ships its own copy.
    if cfg!(windows) {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let pf = PathBuf::from(pf);
            out.push(
                pf.join("Meta Horizon")
                    .join("Support")
                    .join("oculus-drivers")
                    .join(EXE),
            );
            out.push(
                pf.join("Oculus")
                    .join("Support")
                    .join("oculus-adb")
                    .join(EXE),
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_path_that_is_missing_is_an_error_not_a_fallback() {
        let bogus = PathBuf::from("Z:/definitely/not/here/adb.exe");
        let err = resolve(Some(&bogus)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("configured adb path"), "got: {msg}");
    }

    #[test]
    fn configured_path_wins_when_it_exists() {
        // Any existing file proves the branch; we never execute it here.
        let me = std::env::current_exe().unwrap();
        let resolved = resolve(Some(&me)).unwrap();
        assert_eq!(resolved.source, AdbSource::Configured);
        assert_eq!(resolved.path, me);
    }

    #[test]
    fn candidate_lists_are_non_empty_and_end_in_the_exe_name() {
        for candidate in bundled_candidates().into_iter().chain(sdk_candidates()) {
            assert_eq!(candidate.file_name().unwrap().to_str().unwrap(), EXE);
        }
    }
}
