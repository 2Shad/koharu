//! Platform file pickers for import and export.
//!
//! Desktop uses native dialogs. Mobile needs the system document picker from the Android platform
//! plugin; until that lands, the mobile pickers reject the request instead of opening nothing.

use std::path::PathBuf;

use anyhow::Result;
use tauri::WebviewWindow;

use super::lifecycle::PageImportSource;

/// Lets the user pick import inputs.
///
/// `Some` carries the selected paths, `None` means the user cancelled.
#[cfg(desktop)]
pub(crate) async fn pick_import(
    window: &WebviewWindow,
    source: PageImportSource,
    extensions: &[&str],
) -> Result<Option<Vec<PathBuf>>> {
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
        PageImportSource::Folder => dialog
            .pick_folder()
            .await
            .map(|folder| vec![folder.path().to_owned()]),
    };
    Ok(picked)
}

/// Lets the user pick an export destination.
///
/// `file_name` requests a save-file dialog with that suggestion, `None` requests a directory.
#[cfg(desktop)]
pub(crate) async fn pick_export(
    window: &WebviewWindow,
    file_name: Option<&str>,
    filter: Option<(&str, &[&str])>,
) -> Result<Option<PathBuf>> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(window);
    let dialog = match filter {
        Some((name, extensions)) => dialog.add_filter(name, extensions),
        None => dialog,
    };
    let picked = match file_name {
        Some(file_name) => dialog
            .set_file_name(file_name)
            .save_file()
            .await
            .map(|file| file.path().to_owned()),
        None => dialog
            .pick_folder()
            .await
            .map(|folder| folder.path().to_owned()),
    };
    Ok(picked)
}

#[cfg(mobile)]
pub(crate) async fn pick_import(
    _window: &WebviewWindow,
    _source: PageImportSource,
    _extensions: &[&str],
) -> Result<Option<Vec<PathBuf>>> {
    anyhow::bail!("the document picker is unavailable on this platform")
}

#[cfg(mobile)]
pub(crate) async fn pick_export(
    _window: &WebviewWindow,
    _file_name: Option<&str>,
    _filter: Option<(&str, &[&str])>,
) -> Result<Option<PathBuf>> {
    anyhow::bail!("the document picker is unavailable on this platform")
}
