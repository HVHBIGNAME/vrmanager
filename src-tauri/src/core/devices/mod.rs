//! Device domain model — what the UI shows and what profiles persist.

pub mod manager;
pub mod model;
pub mod profile;

pub use manager::{DeviceManager, InstallOutcome, InstalledApp};
pub use model::{ConnectionType, Device, DeviceInfo, DeviceSnapshot};
pub use profile::{DeviceProfile, ProfileStore};
