#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(desktop, target_os = "linux", target_env = "gnu"))]
mod mallinfo;

#[cfg(desktop)]
use clap::Parser as _;

#[cfg(desktop)]
#[derive(clap::Parser)]
#[command(version, about)]
struct Cli {}

/// Desktop process entrypoint. Mobile launches through `koharu::run`'s platform entry
/// point instead, so Cargo's binary target is inert there.
#[cfg(desktop)]
#[tokio::main]
#[tauri_runtime_cef::cef_entry_point]
async fn main() {
    #[cfg(target_os = "windows")]
    {
        // SAFETY: This only requests the existing parent console. It does not allocate one.
        let _ = unsafe {
            windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            )
        };
    }

    let _cli = Cli::parse();
    let _guard = koharu::sentry::initialize();
    koharu::panic::install();
    tokio::task::block_in_place(koharu::run);
}

#[cfg(mobile)]
fn main() {}
