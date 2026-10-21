//! Android platform services owned by Koharu: storage access framework pickers, the share
//! sheet, Keystore-backed secrets, and a foreground service for long-running work.
//!
//! The Kotlin implementation is registered as a Tauri mobile plugin. No command is exposed to
//! the webview: the app calls [`Android`] directly through [`init`].

use tauri::{DynRuntime, Manager as _, plugin::TauriPlugin};

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

#[cfg(not(target_os = "android"))]
pub use desktop::Android;
#[cfg(target_os = "android")]
pub use mobile::Android;

/// A destination previously returned by `pick_export_directory` or `pick_export_file`.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "uri")]
pub enum ExportTarget {
    Directory(String),
    File(String),
}

/// Initializes the plugin and manages [`Android`] as app state.
pub fn init() -> TauriPlugin<DynRuntime> {
    tauri::plugin::Builder::new("koharu-android")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            app.manage(Android::new(
                api.register_android_plugin("rs.koharu.android", "KoharuPlugin")?,
            ));
            #[cfg(not(target_os = "android"))]
            {
                let _ = api;
                app.manage(Android);
            }
            Ok(())
        })
        .build()
}
