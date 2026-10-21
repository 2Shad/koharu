//! Platform file pickers for import and export.
//!
//! Desktop opens native dialogs. Android returns cached local copies from the system document
//! picker and opaque content URIs for export destinations.

use std::path::PathBuf;

use anyhow::Result;
use tauri::WebviewWindow;

#[cfg(target_os = "android")]
use anyhow::Context as _;
#[cfg(target_os = "android")]
use tauri::Manager as _;

use super::lifecycle::PageImportSource;

/// Where an export is written. Desktop destinations are direct paths; Android destinations are
/// opaque content URIs that the platform plugin publishes into.
pub(crate) enum Destination {
    #[cfg(desktop)]
    Directory(PathBuf),
    #[cfg(desktop)]
    File(PathBuf),
    #[cfg(target_os = "android")]
    Tree(String),
    #[cfg(target_os = "android")]
    Document(String),
}

/// Lets the user pick import inputs.
///
/// `Some` carries the selected local files, `None` means the user cancelled. A folder selection
/// is expanded on desktop, so both platforms return files.
#[cfg(desktop)]
pub(crate) async fn pick_import(
    window: &WebviewWindow,
    source: PageImportSource,
    extensions: &[&str],
) -> Result<Option<Vec<PathBuf>>> {
    use walkdir::WalkDir;

    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("Images, archives, and PDF", extensions)
        .set_parent(window);
    let picked = match source {
        PageImportSource::Files => dialog.pick_files().await.map(|files| {
            files
                .into_iter()
                .map(|file| file.path().to_owned())
                .collect::<Vec<_>>()
        }),
        PageImportSource::Folder => dialog.pick_folder().await.map(|folder| {
            WalkDir::new(folder.path())
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| match entry {
                    Ok(entry) if entry.file_type().is_file() => Some(entry.into_path()),
                    Ok(_) => None,
                    Err(error) => {
                        tracing::warn!(%error, "could not inspect an import directory entry");
                        None
                    }
                })
                .filter(|path| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.parse::<super::import::Format>().is_ok())
                })
                .collect::<Vec<_>>()
        }),
    };
    Ok(picked)
}

#[cfg(target_os = "android")]
pub(crate) async fn pick_import(
    window: &WebviewWindow,
    _source: PageImportSource,
    extensions: &[&str],
) -> Result<Option<Vec<PathBuf>>> {
    android(window)?.pick_import(extensions).await
}

#[cfg(all(mobile, not(target_os = "android")))]
pub(crate) async fn pick_import(
    _window: &WebviewWindow,
    _source: PageImportSource,
    _extensions: &[&str],
) -> Result<Option<Vec<PathBuf>>> {
    anyhow::bail!("the document picker is unavailable on this platform")
}

/// Lets the user pick an export destination.
///
/// `file` carries a suggested file name and MIME type for a save-file request; `None` requests a
/// directory. `filter` is the desktop dialog filter for that save-file request.
#[cfg(desktop)]
pub(crate) async fn pick_export(
    window: &WebviewWindow,
    _project_name: &str,
    file: Option<(&str, &str)>,
    filter: Option<(&str, &[&str])>,
) -> Result<Option<Destination>> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(window);
    let picked = match file {
        Some((file_name, _)) => {
            let dialog = match filter {
                Some((label, extensions)) => dialog.add_filter(label, extensions),
                None => dialog,
            };
            dialog
                .set_file_name(file_name)
                .save_file()
                .await
                .map(|file| Destination::File(file.path().to_owned()))
        }
        None => dialog
            .pick_folder()
            .await
            .map(|folder| Destination::Directory(folder.path().to_owned())),
    };
    Ok(picked)
}

#[cfg(target_os = "android")]
pub(crate) async fn pick_export(
    window: &WebviewWindow,
    project_name: &str,
    file: Option<(&str, &str)>,
    _filter: Option<(&str, &[&str])>,
) -> Result<Option<Destination>> {
    let android = android(window)?;
    Ok(match file {
        Some((file_name, mime)) => android
            .pick_export_file(file_name, mime)
            .await?
            .map(Destination::Document),
        None => android
            .pick_export_directory(project_name)
            .await?
            .map(Destination::Tree),
    })
}

#[cfg(all(mobile, not(target_os = "android")))]
pub(crate) async fn pick_export(
    _window: &WebviewWindow,
    _project_name: &str,
    _file: Option<(&str, &str)>,
    _filter: Option<(&str, &[&str])>,
) -> Result<Option<Destination>> {
    anyhow::bail!("the document picker is unavailable on this platform")
}

#[cfg(target_os = "android")]
fn android(window: &WebviewWindow) -> Result<koharu_android::Android> {
    Ok(window
        .app_handle()
        .try_state::<koharu_android::Android>()
        .context("the Android platform plugin state is unavailable")?
        .inner()
        .clone())
}
