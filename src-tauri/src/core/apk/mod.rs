//! APK inspection.
//!
//! Metadata must be available *before* installing (product requirement), so
//! VRManager parses the APK itself: a ZIP container plus Android's binary XML
//! ("AXML") encoding of `AndroidManifest.xml`. No `aapt` dependency — the
//! desktop app must work on a machine without the Android SDK.

// Budget the manifest, not the APK: VR assets can legitimately be many GiB.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

pub mod axml;
pub mod meta;

pub use meta::{inspect_apk, ApkMetadata};
