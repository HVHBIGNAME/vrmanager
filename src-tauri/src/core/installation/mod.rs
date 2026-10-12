//! Installation: queueing, scheduling and result aggregation.
//!
//! The queue lives in the core (not the UI) so that work already touching a
//! device survives a window close, and so the aggregate outcome of a batch is
//! decided by the layer that observed every item.

pub mod input;
pub mod queue;
pub(crate) mod runner;
mod snapshot;

#[cfg(test)]
mod runner_tests;

pub use queue::{
    InstallItemSnapshot, InstallJobSnapshot, InstallQueue, ItemStatus, JobStatus, QueuedInstall,
    SharedQueue,
};
