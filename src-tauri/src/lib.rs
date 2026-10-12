//! VRManager desktop — library root.
//!
//! Layering (dependencies point inward only):
//!
//! ```text
//! React UI
//!    ↓  Tauri commands/events
//! commands/   state/   events/
//!    ↓
//! core::devices::DeviceManager
//!    ↓                    ↓
//! core::platform     core::apk
//!    ↓
//! core::adb  →  Pico / Quest / Android
//! ```
//!
//! `core` never depends on Tauri, so the whole device/APK layer is testable
//! without a window and without hardware.

pub mod commands;
pub mod core;
pub mod error;
pub mod events;
pub mod state;

pub use error::{CoreError, UiError};

/// Build and run the desktop application.
pub fn run() {
    init_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_environment,
            commands::get_settings,
            commands::set_adb_path,
            commands::set_show_system_apps,
            commands::set_selected_devices,
            commands::complete_onboarding,
            commands::list_devices,
            commands::get_device,
            commands::rename_device,
            commands::enable_wireless,
            commands::connect_wireless,
            commands::disconnect_wireless,
            commands::list_apps,
            commands::get_app_version,
            commands::launch_app,
            commands::force_stop_app,
            commands::uninstall_app,
            commands::clear_app_data,
            commands::inspect_apks,
            commands::inspect_apk_batch,
            commands::check_apk_compatibility,
            commands::install_apk,
            commands::start_install_queue,
            commands::retry_install_job,
            commands::get_install_job,
            commands::list_install_jobs,
            commands::cancel_job,
            commands::device_capabilities,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VRManager");
}

/// Structured logging; every ADB/install operation is traceable.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "vrmanager=debug,info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}
