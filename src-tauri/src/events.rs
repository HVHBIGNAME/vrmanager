//! Typed event names emitted to the UI.
//!
//! Device and job state changes are pushed, not polled: the product must not
//! scan ADB on a timer (performance requirement), and React state stays in
//! sync through these events.

use serde::Serialize;

/// Channel names. Keep in sync with `src/lib/events.ts`.
pub mod channel {
    pub const DEVICES_CHANGED: &str = "vrmanager://devices-changed";
    /// Whole-snapshot updates for an installation job — the ONE source of
    /// truth for progress. There is deliberately no second per-line event.
    pub const INSTALL_JOB_CHANGED: &str = "vrmanager://install-job-changed";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallStage {
    Preparing,
    Transferring,
    Installing,
    Verifying,
    Done,
    Failed,
}

/// Interpret an adb progress line into a user-facing stage + percentage.
///
/// adb prints lines like:
///   `foo.apk: 1 file pushed, 0 skipped. 42.1 MB/s (124000000 bytes in 2.9s)`
///   `[  3%] /data/local/tmp/foo.apk`
///   `Performing Streamed Install`
///   `Success`
pub fn classify_progress(line: &str) -> (InstallStage, Option<u8>) {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();

    // `[ 42%]` style progress.
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed[start..].find('%') {
            let inner = &trimmed[start + 1..start + end];
            if let Ok(pct) = inner.trim().parse::<u8>() {
                return (InstallStage::Transferring, Some(pct.min(100)));
            }
        }
    }

    if lower.contains("success") {
        return (InstallStage::Done, Some(100));
    }
    if lower.contains("failure") || lower.contains("install_failed") || lower.contains("error") {
        return (InstallStage::Failed, None);
    }
    if lower.contains("streamed install") || lower.contains("performing") {
        return (InstallStage::Installing, None);
    }
    if lower.contains("pushed") || lower.contains("bytes in") {
        return (InstallStage::Installing, None);
    }

    (InstallStage::Preparing, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_percentage_lines() {
        let (stage, pct) = classify_progress("[  7%] /data/local/tmp/app.apk");
        assert_eq!(stage, InstallStage::Transferring);
        assert_eq!(pct, Some(7));

        let (_, pct) = classify_progress("[ 100%] /data/local/tmp/app.apk");
        assert_eq!(pct, Some(100));
    }

    #[test]
    fn recognises_terminal_states() {
        assert_eq!(classify_progress("Success").0, InstallStage::Done);
        assert_eq!(
            classify_progress("Failure [INSTALL_FAILED_INSUFFICIENT_STORAGE]").0,
            InstallStage::Failed
        );
    }

    #[test]
    fn recognises_streaming_and_push_phases() {
        assert_eq!(
            classify_progress("Performing Streamed Install").0,
            InstallStage::Installing
        );
        assert_eq!(
            classify_progress("app.apk: 1 file pushed, 0 skipped. 42.1 MB/s").0,
            InstallStage::Installing
        );
    }
}
