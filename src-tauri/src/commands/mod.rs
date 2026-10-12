//! Tauri command surface — the only API React is allowed to call.
//!
//! Every command returns `Result<T, UiError>`: the UI always receives a
//! human-readable message plus optional technical details, never a panic or a
//! raw shell string.

#[cfg(test)]
#[path = "apk_tests.rs"]
mod apk_tests;

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::core::adb::binary::AdbSource;
use crate::core::apk::{inspect_apk, ApkMetadata};
use crate::core::cancel::CancelToken;
use crate::core::devices::manager::InstalledApp;
use crate::core::devices::model::DeviceSnapshot;
use crate::core::installation::runner::{InstallFuture, Installer, Publish};
use crate::core::installation::{InstallJobSnapshot, QueuedInstall};
use crate::core::platform::capability::Capability;
use crate::error::{CoreError, UiError, UiResult};
use crate::events::channel;
use crate::state::{AppState, Settings};

/// Environment report for the Settings screen and first-run flow.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub adb_available: bool,
    pub adb_path: Option<String>,
    pub adb_source: Option<AdbSource>,
    pub adb_version: Option<String>,
    pub backend_configured: bool,
    pub onboarded: bool,
}

#[tauri::command]
pub async fn get_environment(state: State<'_, AppState>) -> UiResult<Environment> {
    let settings = state.settings().await;

    let (adb_available, adb_path, adb_source, adb_version) = match state.manager().await {
        Ok(manager) => {
            let info = state.adb_info().await;
            let version = manager.client().version(&CancelToken::new()).await.ok();
            (
                true,
                info.as_ref().map(|i| i.path.display().to_string()),
                info.as_ref().map(|i| i.source),
                version,
            )
        }
        Err(_) => (false, None, None, None),
    };

    Ok(Environment {
        adb_available,
        adb_path,
        adb_source,
        adb_version,
        backend_configured: settings.backend_url.is_some(),
        onboarded: settings.onboarded,
    })
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> UiResult<Settings> {
    Ok(state.settings().await)
}

#[tauri::command]
pub async fn set_adb_path(state: State<'_, AppState>, path: Option<String>) -> UiResult<Settings> {
    let settings = state
        .update_settings(|s| s.adb_path = path.map(PathBuf::from))
        .await?;
    state.reset_manager().await;
    Ok(settings)
}

#[tauri::command]
pub async fn set_show_system_apps(state: State<'_, AppState>, show: bool) -> UiResult<Settings> {
    Ok(state.update_settings(|s| s.show_system_apps = show).await?)
}

#[tauri::command]
pub async fn set_selected_devices(
    state: State<'_, AppState>,
    serials: Vec<String>,
) -> UiResult<Settings> {
    Ok(state
        .update_settings(|s| s.selected_serials = serials)
        .await?)
}

#[tauri::command]
pub async fn complete_onboarding(state: State<'_, AppState>) -> UiResult<Settings> {
    Ok(state.update_settings(|s| s.onboarded = true).await?)
}

// ---- devices ---------------------------------------------------------------

#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> UiResult<Vec<DeviceSnapshot>> {
    let manager = state.manager().await?;
    let devices = manager.list_devices(&CancelToken::new()).await?;
    Ok(devices.iter().map(DeviceSnapshot::from).collect())
}

/// Full detail for the device dashboard (battery, storage, capabilities).
#[tauri::command]
pub async fn get_device(
    state: State<'_, AppState>,
    serial: String,
) -> UiResult<crate::core::devices::model::Device> {
    let manager = state.manager().await?;
    match manager.cached(&serial).await {
        Some(device) => Ok(device),
        None => {
            // Not scanned yet in this session: scan once, then look again.
            manager.list_devices(&CancelToken::new()).await?;
            manager
                .cached(&serial)
                .await
                .ok_or_else(|| UiError::from(CoreError::DeviceNotFound(serial)))
        }
    }
}

#[tauri::command]
pub async fn rename_device(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
    name: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    manager.rename_device(&serial, &name).await?;
    let _ = app.emit(channel::DEVICES_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub async fn enable_wireless(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
) -> UiResult<String> {
    let manager = state.manager().await?;
    let addr = manager
        .enable_wireless(&serial, &CancelToken::new())
        .await?;
    let _ = app.emit(channel::DEVICES_CHANGED, ());
    Ok(addr)
}

#[tauri::command]
pub async fn connect_wireless(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
) -> UiResult<String> {
    let manager = state.manager().await?;
    let addr = manager
        .connect_wireless(&address, &CancelToken::new())
        .await?;
    let _ = app.emit(channel::DEVICES_CHANGED, ());
    Ok(addr)
}

#[tauri::command]
pub async fn disconnect_wireless(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    manager
        .disconnect_wireless(&address, &CancelToken::new())
        .await?;
    let _ = app.emit(channel::DEVICES_CHANGED, ());
    Ok(())
}

// ---- library ---------------------------------------------------------------

#[tauri::command]
pub async fn list_apps(state: State<'_, AppState>, serial: String) -> UiResult<Vec<InstalledApp>> {
    let manager = state.manager().await?;
    let include_system = state.settings().await.show_system_apps;
    Ok(manager
        .list_apps(&serial, include_system, &CancelToken::new())
        .await?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersionInfo {
    pub version_name: Option<String>,
    pub version_code: Option<u64>,
}

#[tauri::command]
pub async fn get_app_version(
    state: State<'_, AppState>,
    serial: String,
    package: String,
) -> UiResult<AppVersionInfo> {
    let manager = state.manager().await?;
    let (version_name, version_code) = manager
        .app_version(&serial, &package, &CancelToken::new())
        .await?;
    Ok(AppVersionInfo {
        version_name,
        version_code,
    })
}

#[tauri::command]
pub async fn launch_app(
    state: State<'_, AppState>,
    serial: String,
    package: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    Ok(manager
        .launch_app(&serial, &package, &CancelToken::new())
        .await?)
}

#[tauri::command]
pub async fn force_stop_app(
    state: State<'_, AppState>,
    serial: String,
    package: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    Ok(manager
        .force_stop(&serial, &package, &CancelToken::new())
        .await?)
}

#[tauri::command]
pub async fn uninstall_app(
    state: State<'_, AppState>,
    serial: String,
    package: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    Ok(manager
        .uninstall_app(&serial, &package, &CancelToken::new())
        .await?)
}

#[tauri::command]
pub async fn clear_app_data(
    state: State<'_, AppState>,
    serial: String,
    package: String,
) -> UiResult<()> {
    let manager = state.manager().await?;
    Ok(manager
        .clear_data(&serial, &package, &CancelToken::new())
        .await?)
}

// ---- apk -------------------------------------------------------------------

/// Inspect dropped/picked APK files. Parsing happens off the UI thread.
#[tauri::command]
pub async fn inspect_apks(paths: Vec<String>) -> UiResult<Vec<ApkMetadata>> {
    let paths = crate::core::installation::input::apk_paths(paths)?;
    let handles: Vec<_> = paths
        .into_iter()
        .map(|p| tokio::task::spawn_blocking(move || inspect_apk(std::path::Path::new(&p))))
        .collect();

    let mut out = Vec::with_capacity(handles.len());
    let mut first_error: Option<UiError> = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(meta)) => out.push(meta),
            Ok(Err(e)) => first_error = first_error.or_else(|| Some(e.into())),
            Err(e) => {
                first_error =
                    first_error.or_else(|| Some(UiError::from(CoreError::other(e.to_string()))))
            }
        }
    }

    // Partial success is still useful (user dropped 7 files, 1 is broken).
    if out.is_empty() {
        if let Some(err) = first_error {
            return Err(err);
        }
    }
    Ok(out)
}

/// Inspect dropped/picked APK files, reporting per-file failures instead of
/// hiding them: a user who drops 7 files must see which one is broken.
#[tauri::command]
pub async fn inspect_apk_batch(paths: Vec<String>) -> UiResult<ApkBatch> {
    let paths = crate::core::installation::input::apk_paths(paths)?;
    let handles: Vec<_> = paths
        .into_iter()
        .map(|p| {
            let path_for_error = p.clone();
            (
                path_for_error,
                tokio::task::spawn_blocking(move || inspect_apk(std::path::Path::new(&p))),
            )
        })
        .collect();

    let mut apks = Vec::new();
    let mut errors = Vec::new();
    for (path, handle) in handles {
        match handle.await {
            Ok(Ok(meta)) => apks.push(meta),
            Ok(Err(e)) => errors.push(ApkBatchError {
                path,
                error: e.into(),
            }),
            Err(e) => errors.push(ApkBatchError {
                path,
                error: UiError::from(CoreError::other(e.to_string())),
            }),
        }
    }
    Ok(ApkBatch { apks, errors })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkBatch {
    pub apks: Vec<ApkMetadata>,
    pub errors: Vec<ApkBatchError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkBatchError {
    pub path: String,
    pub error: UiError,
}

/// Pre-flight compatibility check shown on the install sheet.
#[tauri::command]
pub async fn check_apk_compatibility(
    state: State<'_, AppState>,
    serial: String,
    apk_path: String,
) -> UiResult<Vec<String>> {
    let manager = state.manager().await?;
    let path = apk_path.clone();
    let meta = tokio::task::spawn_blocking(move || inspect_apk(std::path::Path::new(&path)))
        .await
        .map_err(|e| UiError::from(CoreError::other(e.to_string())))??;
    Ok(manager.check_compatibility(&serial, &meta).await)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallJob {
    pub job_id: String,
}

/// Start a queue: every APK on every selected device.
///
/// Returns as soon as the job exists; the work keeps running in the core even
/// if the window that started it is closed. Progress arrives through
/// `vrmanager://install-job-changed` as whole snapshots.
#[tauri::command]
pub async fn start_install_queue(
    app: AppHandle,
    state: State<'_, AppState>,
    serials: Vec<String>,
    apk_paths: Vec<String>,
) -> UiResult<InstallJob> {
    let serials = dedupe(serials);
    let apk_paths = dedupe(apk_paths);

    if serials.is_empty() {
        return Err(UiError::new(
            "noDeviceSelected",
            "Choose at least one device to install to.",
        ));
    }
    if apk_paths.is_empty() {
        return Err(UiError::new(
            "noApkSelected",
            "Choose at least one app to install.",
        ));
    }

    // APKs in order, devices in order for each APK.
    let mut items = Vec::new();
    for path in &apk_paths {
        for serial in &serials {
            items.push(QueuedInstall {
                item_id: uuid::Uuid::new_v4().to_string(),
                serial: serial.clone(),
                apk_path: PathBuf::from(path),
            });
        }
    }

    spawn_queue(app, state, items).await
}

/// Re-run only the items that can plausibly succeed on a second attempt.
#[tauri::command]
pub async fn retry_install_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> UiResult<InstallJob> {
    let items = state.queue().retryable_items(&job_id).await;
    if items.is_empty() {
        return Err(UiError::new(
            "nothingToRetry",
            "There is nothing left to retry in this installation.",
        ));
    }
    spawn_queue(app, state, items).await
}

#[tauri::command]
pub async fn get_install_job(
    state: State<'_, AppState>,
    job_id: String,
) -> UiResult<InstallJobSnapshot> {
    state
        .queue()
        .get(&job_id)
        .await
        .ok_or_else(|| UiError::new("jobNotFound", "That installation is no longer available."))
}

#[tauri::command]
pub async fn list_install_jobs(state: State<'_, AppState>) -> UiResult<Vec<InstallJobSnapshot>> {
    Ok(state.queue().list().await)
}

/// Remove duplicates while preserving the order the user chose.
fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

/// Production installer: real device work, owned by the core runner.
struct ManagerInstaller(std::sync::Arc<crate::core::devices::manager::DeviceManager>);

impl Installer for ManagerInstaller {
    fn prepare(&self, path: &std::path::Path) -> crate::error::CoreResult<ApkMetadata> {
        inspect_apk(path)
    }

    fn install<'a>(
        &'a self,
        serial: &'a str,
        apk: &'a ApkMetadata,
        cancel: &'a CancelToken,
        progress: &'a mut (dyn FnMut(&str) + Send),
    ) -> InstallFuture<'a> {
        Box::pin(async move { self.0.install_apk(serial, apk, cancel, progress).await })
    }
}

/// Create the job and hand it to the core runner.
///
/// The runner owns scheduling, so this returns as soon as the job exists; the
/// work continues even if the window (or the whole progress sheet) goes away.
async fn spawn_queue(
    app: AppHandle,
    state: State<'_, AppState>,
    items: Vec<QueuedInstall>,
) -> UiResult<InstallJob> {
    let manager = state.manager().await?;
    let emitter = app.clone();
    let publish: Publish = std::sync::Arc::new(move |snapshot: InstallJobSnapshot| {
        let _ = emitter.emit(channel::INSTALL_JOB_CHANGED, snapshot);
    });

    let (snapshot, finished) = state
        .install_runner()
        .start(
            std::sync::Arc::new(ManagerInstaller(manager)),
            items,
            publish,
        )
        .await?;

    // Devices may have changed state (installed/uninstalled): refresh the UI.
    let done = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = finished.await;
        let _ = done.emit(channel::DEVICES_CHANGED, ());
    });

    Ok(InstallJob {
        job_id: snapshot.job_id,
    })
}

/// Compatibility wrapper: one APK, one queue.
#[tauri::command]
pub async fn install_apk(
    app: AppHandle,
    state: State<'_, AppState>,
    serials: Vec<String>,
    apk_path: String,
) -> UiResult<InstallJob> {
    start_install_queue(app, state, serials, vec![apk_path]).await
}

/// Cancel a running installation. Terminal or unknown jobs are reported
/// honestly rather than silently accepted.
#[tauri::command]
pub async fn cancel_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> UiResult<()> {
    if let Some(snapshot) = state.queue().cancel(&job_id).await {
        let _ = app.emit(channel::INSTALL_JOB_CHANGED, snapshot);
        return Ok(());
    }
    // Fall back to the legacy registry for non-install jobs.
    Ok(state.cancel_job(&job_id).await?)
}

/// Capabilities for the current device, so the UI can disable unsupported
#[tauri::command]
pub async fn device_capabilities(
    state: State<'_, AppState>,
    serial: String,
) -> UiResult<Vec<&'static str>> {
    let manager = state.manager().await?;
    let device = manager
        .cached(&serial)
        .await
        .ok_or_else(|| UiError::from(CoreError::DeviceNotFound(serial.clone())))?;
    Ok(device.capabilities.iter().map(Capability::as_str).collect())
}

// The install sheet reads job state through `get_install_job` /
// `list_install_jobs`; there is deliberately no placeholder command.
