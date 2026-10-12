//! VRManager application core.
//!
//! Nothing in here knows about Tauri, React or any particular headset vendor.
//! The dependency direction is strictly inward:
//!
//! ```text
//! commands/  ->  core::devices  ->  core::platform  ->  core::adb
//!                      \->  core::apk
//! ```

pub mod adb;
pub mod apk;
pub mod cancel;
pub mod devices;
pub mod installation;
pub mod platform;
