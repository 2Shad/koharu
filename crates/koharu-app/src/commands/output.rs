use anyhow::{Context as _, Result};
use futures::future::try_join_all;
use image::{
    ExtendedColorType, ImageEncoder as _,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use koharu_psd::{PsdExportOptions, export_page};
use koharu_rasterizer::{Raster, RasterOptions, Rasterizer};
use koharu_renderer::{Frame, Renderer};
use koharu_scene::{AssetRole, EntityId, Snapshot};
use rayon::prelude::*;
use serde::Deserialize;
use specta::Type;
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{State, WebviewWindow, ipc::IpcResponse};

use super::{Error, dialogs, project::CurrentProject};
use koharu_desktop::Desktop;

const THUMBNAIL_EDGE: u32 = 128;

#[derive(Type)]
#[specta(transparent)]
pub(crate) struct ThumbnailBytes(#[specta(type = Vec<u8>)] Vec<u8>);

impl IpcResponse for ThumbnailBytes {
    fn body(self) -> tauri::Result<tauri::ipc::InvokeResponseBody> {
        Ok(self.0.into())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Png,
    Psd,
    Cbz,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export(
    window: WebviewWindow,
    format: ExportFormat,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
) -> std::result::Result<(), Error> {
    let (name, snapshot) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        (project.name.clone(), project.snapshot())
    };
    let pages = snapshot.pages().map(|page| page.id()).collect::<Vec<_>>();
    if pages.is_empty() {
        return Err(anyhow::anyhow!("there are no pages to export").into());
    }
    let destination = match format {
        ExportFormat::Png | ExportFormat::Psd => {
            dialogs::pick_export(&window, &name, None, None).await?
        }
        ExportFormat::Cbz => {
            let file_name = format!("{name}.cbz");
            dialogs::pick_export(
                &window,
                &name,
                Some((&file_name, "application/vnd.comicbook+zip")),
                Some(("Comic Book Archive", &["cbz"])),
            )
            .await?
        }
    };
    let Some(destination) = destination else {
        return Ok(());
    };
    let renderer = desktop.renderer();
    let rasterizer = desktop.rasterizer().await?;
    let frames = try_join_all(pages.iter().map(|&page| renderer.render(&snapshot, page))).await?;
    let images = match format {
        ExportFormat::Png | ExportFormat::Cbz => {
            tokio_rayon::spawn(move || {
                frames
                    .par_iter()
                    .map(|frame| -> Result<_> {
                        let image = rasterizer
                            .rasterize(&frame.raster_frame()?, RasterOptions::default())?
                            .image;
                        let mut bytes = Vec::new();
                        PngEncoder::new_with_quality(
                            &mut bytes,
                            CompressionType::Best,
                            FilterType::Adaptive,
                        )
                        .write_image(
                            image.as_raw(),
                            image.width(),
                            image.height(),
                            ExtendedColorType::Rgba8,
                        )?;
                        Ok(bytes)
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .await?
        }
        ExportFormat::Psd => {
            let options = PsdExportOptions::default();
            try_join_all(
                frames
                    .iter()
                    .map(|frame| export_page(Arc::clone(&rasterizer), &snapshot, frame, &options)),
            )
            .await?
        }
    };
    store(&window, &name, destination, format, pages, snapshot, images).await?;
    Ok(())
}

/// Writes the encoded pages to the destination.
///
/// Android destinations are content URIs, so the files are staged in the app cache and handed to
/// the platform plugin; desktop destinations are direct paths.
#[cfg(desktop)]
async fn store(
    _window: &WebviewWindow,
    _project_name: &str,
    destination: dialogs::Destination,
    format: ExportFormat,
    pages: Vec<EntityId>,
    snapshot: Snapshot,
    images: Vec<Vec<u8>>,
) -> Result<()> {
    match destination {
        dialogs::Destination::Directory(directory) => {
            tokio_rayon::spawn(move || {
                produce(&directory, format, None, &pages, &snapshot, images).map(drop)
            })
            .await?;
        }
        dialogs::Destination::File(path) => {
            tokio_rayon::spawn(move || -> Result<()> {
                let directory = path.parent().context("archive path has no parent")?;
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("archive path has no file name")?;
                let staging = tempfile::tempdir_in(directory)?;
                let produced = produce(
                    staging.path(),
                    format,
                    Some(file_name),
                    &pages,
                    &snapshot,
                    images,
                )?;
                std::fs::rename(&produced[0], &path)?;
                Ok(())
            })
            .await?;
        }
    }
    Ok(())
}

#[cfg(target_os = "android")]
async fn store(
    window: &WebviewWindow,
    project_name: &str,
    destination: dialogs::Destination,
    format: ExportFormat,
    pages: Vec<EntityId>,
    snapshot: Snapshot,
    images: Vec<Vec<u8>>,
) -> Result<()> {
    use tauri::Manager as _;

    let staging = window
        .path()
        .app_cache_dir()?
        .join(format!("export-{}", uuid::Uuid::new_v4()));
    let result = async {
        let (target, archive) = match &destination {
            dialogs::Destination::Tree(uri) => {
                (koharu_android::ExportTarget::Directory(uri.clone()), None)
            }
            dialogs::Destination::Document(uri) => (
                koharu_android::ExportTarget::File(uri.clone()),
                Some(format!("{project_name}.cbz")),
            ),
        };
        let directory = staging.clone();
        let produced = tokio_rayon::spawn(move || {
            produce(
                &directory,
                format,
                archive.as_deref(),
                &pages,
                &snapshot,
                images,
            )
        })
        .await?;
        let android = window.state::<koharu_android::Android>().inner().clone();
        android.publish_export(&target, produced).await?;
        Ok(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(all(mobile, not(target_os = "android")))]
async fn store(
    _window: &WebviewWindow,
    _project_name: &str,
    _destination: dialogs::Destination,
    _format: ExportFormat,
    _pages: Vec<EntityId>,
    _snapshot: Snapshot,
    _images: Vec<Vec<u8>>,
) -> Result<()> {
    anyhow::bail!("export is unavailable on this platform")
}

/// Writes the encoded pages into `directory`, as one file per page or as one archive when
/// `archive_name` is set. Returns the files it created.
fn produce(
    directory: &Path,
    format: ExportFormat,
    archive_name: Option<&str>,
    pages: &[EntityId],
    snapshot: &Snapshot,
    images: Vec<Vec<u8>>,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(directory)?;
    let extension = match format {
        ExportFormat::Psd => "psd",
        ExportFormat::Png | ExportFormat::Cbz => "png",
    };
    let width = pages.len().to_string().len().max(4);
    // Async preparation and indexed encoding both preserve project order.
    let mut produced = Vec::with_capacity(archive_name.map_or(pages.len(), |_| 1));
    if let Some(archive_name) = archive_name {
        let path = directory.join(archive_name);
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path)?);
        for (index, (page_id, bytes)) in pages.iter().zip(images).enumerate() {
            let name = export_name(snapshot, *page_id, index, width, extension)?;
            // PNG data is already compressed.
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            archive.start_file(name, options)?;
            archive.write_all(&bytes)?;
        }
        archive.finish()?;
        produced.push(path);
    } else {
        for (index, (page_id, bytes)) in pages.iter().zip(images).enumerate() {
            let name = export_name(snapshot, *page_id, index, width, extension)?;
            let path = directory.join(name);
            std::fs::write(&path, bytes)?;
            produced.push(path);
        }
    }
    Ok(produced)
}

fn export_name(
    snapshot: &Snapshot,
    page_id: EntityId,
    index: usize,
    width: usize,
    extension: &str,
) -> Result<String> {
    let page = snapshot.page(page_id)?.page()?;
    let name = page
        .label
        .trim()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    let name = name
        .rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .replace(['<', '>', ':', '"', '/', '\\', '|', '?', '*'], "_");
    Ok(format!(
        "{:0width$}_{}.{extension}",
        index + 1,
        if name.is_empty() { "page" } else { &name }
    ))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_thumbnail(
    page: EntityId,
    project: State<'_, CurrentProject>,
) -> std::result::Result<ThumbnailBytes, Error> {
    let snapshot = project
        .project
        .lock()
        .await
        .as_ref()
        .context("no project is open")?
        .snapshot();
    snapshot.page(page)?;
    let blob = snapshot
        .asset(page, &AssetRole::new("source")?)?
        .with_context(|| format!("page {page} has no source image"))?
        .blob;
    let bytes = snapshot.read_blob(blob).await?;
    let bytes = tokio_rayon::spawn(move || -> Result<Vec<u8>> {
        let image = image::load_from_memory(&bytes).context("failed to decode source image")?;
        if image.width() == 0 || image.height() == 0 {
            return Err(anyhow::anyhow!("source image is empty"));
        }
        let image = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        Ok(encoder.encode(80.0).to_vec())
    })
    .await?;
    Ok(ThumbnailBytes(bytes))
}

pub(crate) async fn rendered_preview(
    renderer: &Renderer,
    rasterizer: Arc<Rasterizer>,
    snapshot: &Snapshot,
    page: EntityId,
) -> Result<Vec<u8>> {
    snapshot.page(page)?;
    let frame = renderer.render(snapshot, page).await?;
    let image = rasterize(rasterizer, &frame, RasterOptions::default())
        .await?
        .image;
    tokio_rayon::spawn(move || {
        let image = image::DynamicImage::ImageRgba8(image)
            .resize(1024, 1024, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        Ok::<_, anyhow::Error>(encoder.encode(85.0).to_vec())
    })
    .await
}

async fn rasterize(
    rasterizer: Arc<Rasterizer>,
    frame: &Frame,
    options: RasterOptions,
) -> Result<Raster> {
    let frame = frame.raster_frame()?;
    tokio_rayon::spawn(move || rasterizer.rasterize(&frame, options))
        .await
        .map_err(Into::into)
}
