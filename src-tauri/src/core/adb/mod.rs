//! Typed ADB client.
//!
//! Everything that actually executes `adb` funnels through here so that
//! timeouts, cancellation, logging and error classification exist in exactly
//! one place. Higher layers never build command lines by hand.

pub mod binary;
pub mod client;
pub mod parse;

pub use binary::{resolve, AdbBinary, AdbSource};
pub use client::{AdbClient, CommandOutput};
pub use parse::{parse_devices, parse_getprop, AdbDeviceEntry, AdbDeviceState};
