//! Serializable installation state; no UI runtime dependencies.
use crate::error::UiError;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ItemStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallItemSnapshot {
    pub item_id: String,
    pub serial: String,
    pub apk_path: String,
    pub package: Option<String>,
    pub application_name: Option<String>,
    pub status: ItemStatus,
    pub stage: crate::events::InstallStage,
    pub percent: Option<u8>,
    pub error: Option<UiError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallJobSnapshot {
    pub job_id: String,
    pub revision: u64,
    pub status: JobStatus,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
    pub items: Vec<InstallItemSnapshot>,
}

impl InstallJobSnapshot {
    pub(super) fn derive_status(items: &[InstallItemSnapshot]) -> JobStatus {
        if items.iter().any(|i| !i.status.is_terminal()) {
            return if items.iter().any(|i| i.status == ItemStatus::Running) {
                JobStatus::Running
            } else {
                JobStatus::Queued
            };
        }
        if items.iter().any(|i| i.status == ItemStatus::Failed) {
            JobStatus::Failed
        } else if items.iter().any(|i| i.status == ItemStatus::Cancelled) {
            JobStatus::Cancelled
        } else {
            JobStatus::Completed
        }
    }

    pub fn is_terminal(&self) -> bool {
        !matches!(self.status, JobStatus::Queued | JobStatus::Running)
    }
}

#[derive(Debug, Clone)]
pub struct QueuedInstall {
    pub item_id: String,
    pub serial: String,
    pub apk_path: PathBuf,
}
