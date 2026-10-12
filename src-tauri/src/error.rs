//! VRManager error model.
//!
//! Two layers on purpose:
//!   * [`CoreError`] — precise, technical, internal.
//!   * [`UiError`]   — what crosses the Tauri boundary: a human sentence plus
//!     optional technical details the UI shows behind an expander.
//!
//! The UI must never render a raw shell/ADB string as its primary message
//! (product requirement), so the mapping lives here instead of in React.

use std::time::Duration;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("adb executable not found")]
    AdbNotFound,

    #[error("adb exited with code {code:?}: {stderr}")]
    AdbFailed {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },

    #[error("adb could not be launched: {0}")]
    AdbSpawn(String),

    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    #[error("operation cancelled")]
    Cancelled,

    #[error("device {0} is not connected")]
    DeviceNotFound(String),

    #[error("device {0} has not authorized this computer")]
    Unauthorized(String),

    #[error("device {serial} is in state `{state}`")]
    DeviceUnavailable { serial: String, state: String },

    #[error("device does not support `{0}`")]
    Unsupported(&'static str),

    #[error("installation failed: {code}")]
    InstallFailed { code: String, raw: String },

    #[error("installed package {package} was not found on the device")]
    InstallNotVerified { package: String },

    #[error("apk could not be read: {0}")]
    Apk(String),

    #[error("wireless adb could not be enabled: {0}")]
    Wireless(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("data error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// Serializable, user-facing error. `message` is a complete human sentence,
/// `details` is the raw technical text (collapsed in the UI by default).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Stable machine key so the UI can special-case a few flows.
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Hint for the UI: is offering a `Retry` button sensible?
    pub retryable: bool,
}

impl UiError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
            retryable: false,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        let text = details.into();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            self.details = Some(trimmed.to_string());
        }
        self
    }

    pub fn retryable(mut self, yes: bool) -> Self {
        self.retryable = yes;
        self
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<CoreError> for UiError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::AdbNotFound => UiError::new(
                "adbNotFound",
                "VRManager could not find the Android platform tools it needs to talk to your \
                 headset. Set the ADB path in Settings, or install the bundled tools.",
            ),

            CoreError::AdbSpawn(ref raw) => UiError::new(
                "adbSpawn",
                "VRManager could not start the device service on this computer.",
            )
            .with_details(raw.clone())
            .retryable(true),

            CoreError::AdbFailed {
                code,
                ref stdout,
                ref stderr,
            } => {
                let raw = format!("exit: {code:?}\nstdout: {stdout}\nstderr: {stderr}");
                let haystack = format!("{stdout}\n{stderr}").to_ascii_lowercase();
                if haystack.contains("device unauthorized") {
                    UiError::new(
                        "unauthorized",
                        "Your headset has not trusted this computer yet. Put the headset on and \
                         accept the USB debugging prompt, then try again.",
                    )
                    .with_details(raw)
                    .retryable(true)
                } else if haystack.contains("device offline") {
                    UiError::new(
                        "offline",
                        "The headset went offline. Reconnect the cable (or re-pair over Wi-Fi) \
                         and try again.",
                    )
                    .with_details(raw)
                    .retryable(true)
                } else if haystack.contains("no devices") || haystack.contains("device not found") {
                    UiError::new("deviceNotFound", "That device is no longer connected.")
                        .with_details(raw)
                        .retryable(true)
                } else {
                    UiError::new(
                        "deviceCommandFailed",
                        "The device refused the last operation.",
                    )
                    .with_details(raw)
                    .retryable(true)
                }
            }

            CoreError::Timeout(d) => UiError::new(
                "timeout",
                format!(
                    "The headset stopped responding after {} seconds.",
                    d.as_secs().max(1)
                ),
            )
            .retryable(true),

            CoreError::Cancelled => UiError::new("cancelled", "Operation cancelled."),

            CoreError::DeviceNotFound(serial) => {
                UiError::new("deviceNotFound", "That device is no longer connected.")
                    .with_details(format!("serial: {serial}"))
            }

            CoreError::Unauthorized(serial) => UiError::new(
                "unauthorized",
                "Your headset has not trusted this computer yet. Put the headset on and accept \
                 the USB debugging prompt.",
            )
            .with_details(format!("serial: {serial}"))
            .retryable(true),

            CoreError::DeviceUnavailable { serial, state } => UiError::new(
                "deviceUnavailable",
                "The headset is connected but not ready for commands yet.",
            )
            .with_details(format!("serial: {serial}\nstate: {state}"))
            .retryable(true),

            CoreError::Unsupported(what) => {
                UiError::new("unsupported", "This device does not support that action.")
                    .with_details(format!("capability: {what}"))
            }

            CoreError::InstallFailed { code, raw } => {
                UiError::new("installFailed", install_message(&code))
                    .with_details(format!("{code}\n\n{raw}"))
                    .retryable(install_retryable(&code))
            }

            CoreError::InstallNotVerified { package } => UiError::new(
                "installNotVerified",
                "The headset reported the install finished, but the app is not there. \
                 Connect the headset and try again.",
            )
            .with_details(format!("package: {package}"))
            .retryable(true),

            CoreError::Apk(raw) => UiError::new(
                "apkInvalid",
                "That file does not look like a valid Android app package.",
            )
            .with_details(raw),

            CoreError::Wireless(raw) => UiError::new(
                "wirelessFailed",
                "VRManager could not switch this headset to Wi-Fi.",
            )
            .with_details(raw)
            .retryable(true),

            CoreError::Io(e) => UiError::new("io", "A file operation failed on this computer.")
                .with_details(e.to_string())
                .retryable(true),

            CoreError::Serde(e) => {
                UiError::new("data", "VRManager could not read its own saved data.")
                    .with_details(e.to_string())
            }

            CoreError::Other(raw) => {
                UiError::new("unknown", "Something went wrong.").with_details(raw)
            }
        }
    }
}

/// Human translation of the `INSTALL_FAILED_*` family. Anything unknown still
/// gets a sane sentence, with the raw code available in `details`.
fn install_message(code: &str) -> String {
    let c = code.to_ascii_uppercase();
    let msg = if c.contains("INSUFFICIENT_STORAGE") {
        "The headset does not have enough free storage for this app."
    } else if c.contains("ALREADY_EXISTS") {
        "This app is already installed on the headset."
    } else if c.contains("VERSION_DOWNGRADE") {
        "A newer version of this app is already installed. Uninstall it first to go back."
    } else if c.contains("UPDATE_INCOMPATIBLE") || c.contains("INCONSISTENT_CERTIFICATES") {
        "An existing copy of this app was signed by a different developer. Uninstall the old one \
         first."
    } else if c.contains("NO_MATCHING_ABIS") {
        "This app was built for a different processor than the headset uses."
    } else if c.contains("OLDER_SDK") {
        "This app needs a newer Android version than the headset runs."
    } else if c.contains("INVALID_APK") || c.contains("INVALID_URI") {
        "The app package is damaged or incomplete."
    } else if c.contains("USER_RESTRICTED") {
        "The headset blocked the install. Check that USB debugging and app installs are allowed."
    } else if c.contains("VERIFICATION_FAILURE") || c.contains("VERIFICATION_TIMEOUT") {
        "The headset's security check rejected this app package."
    } else if c.contains("ABORTED") {
        "The install was aborted on the headset."
    } else if c.contains("TEST_ONLY") {
        "This package is marked test-only and cannot be installed normally."
    } else {
        "The device rejected the app package."
    };
    msg.to_string()
}

fn install_retryable(code: &str) -> bool {
    let c = code.to_ascii_uppercase();
    !(c.contains("NO_MATCHING_ABIS")
        || c.contains("OLDER_SDK")
        || c.contains("INVALID_APK")
        || c.contains("TEST_ONLY")
        || c.contains("VERSION_DOWNGRADE"))
}

pub type CoreResult<T> = Result<T, CoreError>;
pub type UiResult<T> = Result<T, UiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_adb_output_maps_to_friendly_message() {
        let err = CoreError::AdbFailed {
            code: Some(1),
            stdout: String::new(),
            stderr: "error: device unauthorized.\nThis adb server's $ADB_VENDOR_KEYS...".into(),
        };
        let ui: UiError = err.into();
        assert_eq!(ui.kind, "unauthorized");
        assert!(ui.retryable);
        assert!(ui.details.is_some(), "technical details must be preserved");
        assert!(
            !ui.message.contains("adb"),
            "user-facing copy must not leak tool names: {}",
            ui.message
        );
    }

    #[test]
    fn install_failures_are_translated_and_classified() {
        let ui: UiError = CoreError::InstallFailed {
            code: "INSTALL_FAILED_INSUFFICIENT_STORAGE".into(),
            raw: "adb: failed to install foo.apk".into(),
        }
        .into();
        assert_eq!(ui.kind, "installFailed");
        assert!(ui.message.contains("free storage"));
        assert!(ui.retryable);

        let ui: UiError = CoreError::InstallFailed {
            code: "INSTALL_FAILED_NO_MATCHING_ABIS".into(),
            raw: String::new(),
        }
        .into();
        assert!(!ui.retryable, "ABI mismatch can never succeed on retry");
    }

    #[test]
    fn unknown_install_code_still_gets_a_sentence() {
        let ui: UiError = CoreError::InstallFailed {
            code: "INSTALL_FAILED_WEIRD_NEW_THING".into(),
            raw: "raw".into(),
        }
        .into();
        assert!(ui.message.ends_with('.'));
        assert!(ui.details.unwrap().contains("WEIRD_NEW_THING"));
    }
}
