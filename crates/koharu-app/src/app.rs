use anyhow::{Context as _, Result};
use tauri::{AppHandle, Manager as _, WindowEvent};
use tokio::sync::Mutex;

#[cfg(target_os = "android")]
use koharu_secrets::ExposeSecret as _;

use crate::background::BackgroundWork;
use crate::commands::{
    ChannelExt as _,
    agent::AgentState,
    canvas::CanvasChannel,
    lifecycle::{
        Download, DownloadChannel, DownloadState, Initialization, ProjectChannel, ResourceChannel,
    },
    processing::{JobChannel, Processing},
    project::{CurrentProject, ProjectLibrary},
};
use crate::models::Models;

/// Builds the application with every platform-neutral plugin, command, and window.
///
/// The caller selects the runtime before running it: CEF on desktop, the system webview on
/// mobile. Nothing below this point names a concrete runtime.
pub fn builder() -> tauri::Builder {
    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir { file_name: None },
                ))
                .build(),
        )
        .plugin(tauri_plugin_opener::init());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|handle, _, _| {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .invoke_handler(crate::commands::bindings().invoke_handler())
        .setup(move |application| {
            #[cfg(all(target_os = "windows", not(debug_assertions)))]
            koharu_runtime::Store::configure(
                application
                    .path()
                    .resource_dir()
                    .context("failed to locate Koharu's installation directory")?
                    .join("store"),
            )?;
            #[cfg(mobile)]
            koharu_runtime::Store::configure(application.path().app_data_dir()?.join("runtime"))?;

            let handle = application.handle().clone();
            #[cfg(mobile)]
            koharu_config::configure(application.path().app_config_dir()?.join("config.toml"))?;
            application.manage(CurrentProject {
                project: Mutex::new(None),
            });
            application.manage(ProjectLibrary::new(project_library_root(&handle)?)?);
            application.manage(Processing::default());
            application.manage(CanvasChannel::default());
            application.manage(JobChannel::default());
            application.manage(DownloadChannel::default());
            application.manage(ResourceChannel::default());
            application.manage(ProjectChannel::default());
            application.manage(Initialization::default());

            application.manage(Models::new(handle.clone()));
            application.manage(koharu_desktop::Desktop::new()?);
            application.manage(AgentState::new(handle.clone())?);
            application.manage(BackgroundWork::new(&handle));

            #[cfg(target_os = "android")]
            {
                let android = application
                    .try_state::<koharu_android::Android>()
                    .context("the Android platform plugin state is unavailable")?
                    .inner()
                    .clone();
                koharu_secrets::install(std::sync::Arc::new(AndroidSecrets(android)))?;
            }

            let window_config = application
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .context("the main Tauri window configuration is unavailable")?;
            let window = tauri::WebviewWindowBuilder::from_config(application, window_config)?
                .build()
                .context("failed to create the main window")?;
            window.show().context("failed to show the main window")?;
            window
                .set_focus()
                .context("failed to focus the main window")?;
            handle.state::<Initialization>().ready();

            let mut downloads = koharu_runtime::download::subscribe();
            let download_handle = handle.clone();
            drop(tauri::async_runtime::spawn(async move {
                let mut work = std::collections::HashMap::new();
                loop {
                    match downloads.recv().await {
                        Ok(event) => {
                            let download = match event {
                                koharu_runtime::download::Event::Started { id, name } => {
                                    work.insert(
                                        id,
                                        crate::background::acquire(&download_handle, "", &name),
                                    );
                                    Download {
                                        id,
                                        state: DownloadState::Running,
                                        name: Some(name),
                                        completed: 0,
                                        total: 0,
                                        error: None,
                                    }
                                }
                                koharu_runtime::download::Event::Progress {
                                    id,
                                    name,
                                    completed,
                                    total,
                                } => {
                                    if let Some(work) = work.get(&id) {
                                        work.progress(
                                            "",
                                            &name,
                                            u32::try_from(completed).unwrap_or(u32::MAX),
                                            u32::try_from(total).unwrap_or(u32::MAX),
                                        );
                                    }
                                    Download {
                                        id,
                                        state: DownloadState::Running,
                                        name: Some(name),
                                        completed,
                                        total,
                                        error: None,
                                    }
                                }
                                koharu_runtime::download::Event::Finished { id } => {
                                    work.remove(&id);
                                    Download {
                                        id,
                                        state: DownloadState::Finished,
                                        name: None,
                                        completed: 0,
                                        total: 0,
                                        error: None,
                                    }
                                }
                                koharu_runtime::download::Event::Failed { id, name, error } => {
                                    work.remove(&id);
                                    Download {
                                        id,
                                        state: DownloadState::Failed,
                                        name: Some(name),
                                        completed: 0,
                                        total: 0,
                                        error: Some(error),
                                    }
                                }
                            };
                            let downloads = download_handle.state::<DownloadChannel>();
                            let mut channel = downloads.channel.lock();
                            if let Some(current) = channel.as_ref()
                                && current.send(download).is_err()
                            {
                                channel.take();
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "download channel fell behind");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }));

            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(
                event,
                WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
            ) {
                let processing = window.state::<Processing>();
                for stop in processing.stops.lock().values() {
                    stop.stop();
                }
                processing.stops.lock().clear();
                processing.jobs.lock().clear();
                window.state::<AgentState>().cancel_all();
            }
        })
}

/// Republishes the current canvas so a resumed webview can restore its surface.
pub fn republish_canvas(handle: &AppHandle) {
    let canvas = handle.state::<koharu_desktop::Desktop>().canvas_state();
    handle.state::<CanvasChannel>().channel.publish(canvas);
}

/// Resolves the directory that stores projects.
///
/// Desktop keeps the user-visible `Documents/Koharu` library. Mobile uses application-private
/// storage, which is the only location the platform keeps across upgrades.
fn project_library_root(handle: &AppHandle) -> Result<std::path::PathBuf> {
    #[cfg(desktop)]
    {
        let _ = handle;
        Ok(dirs::document_dir()
            .context("the Documents directory is unavailable")?
            .join("Koharu"))
    }
    #[cfg(mobile)]
    {
        Ok(handle.path().app_data_dir()?.join("library"))
    }
}

/// Credential store backed by the Android Keystore through the platform plugin.
#[cfg(target_os = "android")]
struct AndroidSecrets(koharu_android::Android);

#[cfg(target_os = "android")]
impl koharu_secrets::Backend for AndroidSecrets {
    fn get(&self, key: &str) -> Result<Option<koharu_secrets::SecretString>> {
        Ok(self
            .0
            .secret_get(key)?
            .map(koharu_secrets::SecretString::from))
    }

    fn set(&self, key: &str, secret: &koharu_secrets::SecretString) -> Result<()> {
        self.0.secret_set(key, secret.expose_secret())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.0.secret_delete(key)
    }
}
