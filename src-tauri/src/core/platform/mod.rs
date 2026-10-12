//! Platform abstraction.
//!
//! Vendor differences live *inside* an adapter. The rest of the codebase sees
//! one [`PlatformAdapter`] trait and a [`Capabilities`] set, which keeps
//! `if pico { } else if quest { }` out of the application core.

pub mod adapter;
pub mod capability;
pub mod detect;

pub use adapter::{adapter_for, AndroidAdapter, PicoAdapter, PlatformAdapter, QuestAdapter};
pub use capability::{Capabilities, Capability};
pub use detect::{detect_platform, Platform};
