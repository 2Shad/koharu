//! Koharu's process entrypoint and diagnostics.

pub mod panic;
pub mod sentry;
pub mod tracing;

use tracing_subscriber::layer::SubscriberExt as _;

/// Runs Koharu on the platform runtime: CEF on desktop, the system webview on mobile.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(::tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with(crate::tracing::TimingLayer::new());
    #[cfg(desktop)]
    let registry = registry.with(crate::sentry::tracing_layer());
    ::tracing::subscriber::set_global_default(registry)
        .expect("failed to set the global tracing subscriber");

    // Mobile enters from the system main thread, which is outside Tauri's async runtime.
    // Setup work that spawns background tasks probes `tokio::runtime::Handle::current()`,
    // so hold the runtime context for the lifetime of the app.
    #[cfg(mobile)]
    let async_runtime = tauri::async_runtime::handle();
    #[cfg(mobile)]
    let _runtime = async_runtime.inner().enter();

    let builder = koharu_app::builder();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(koharu_android::init());
    builder
        .runtime(runtime())
        .build(tauri::generate_context!())
        .expect("failed to build the application")
        .run(|handle, event| {
            #[cfg(mobile)]
            handle_event(handle, event);
            #[cfg(not(mobile))]
            let _ = (handle, event);
        });
}

#[cfg(mobile)]
fn handle_event(handle: &tauri::AppHandle, event: tauri::RunEvent) {
    use tauri::{Emitter as _, Manager as _};

    let tauri::RunEvent::WindowEvent { event, .. } = event else {
        return;
    };
    match event {
        tauri::WindowEvent::Resumed => {
            ::tracing::info!("application resumed");
            koharu_app::republish_canvas(handle);
            if let Some(window) = handle.get_webview_window("main")
                && let Err(error) = window.emit("koharu://resumed", ())
            {
                ::tracing::warn!(%error, "failed to announce resume");
            }
        }
        tauri::WindowEvent::Suspended => {
            ::tracing::info!("application suspended");
            if let Some(window) = handle.get_webview_window("main")
                && let Err(error) = window.emit("koharu://suspended", ())
            {
                ::tracing::warn!(%error, "failed to announce suspension");
            }
        }
        _ => {}
    }
}

#[cfg(desktop)]
fn runtime() -> tauri_runtime_cef::Cef {
    let cef = tauri_runtime_cef::Cef::default();
    #[cfg(debug_assertions)]
    let cef = cef.remote_debugging(tauri_runtime_cef::RemoteDebugging::Port {
        port: 4000,
        allowed_origins: Vec::new(),
    });
    #[cfg(target_os = "linux")]
    let cef = cef
        .enable_features(["Vulkan", "VulkanFromANGLE"])
        .command_line_args([
            ("--enable-unsafe-webgpu", None),
            ("use-angle", Some("vulkan")),
            ("--ozone-platform", Some("x11")),
        ]);
    cef
}

#[cfg(mobile)]
fn runtime() -> tauri_runtime_wry::Wry {
    tauri_runtime_wry::Wry::default()
}
