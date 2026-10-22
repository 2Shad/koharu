//! Headless parity harness.
//!
//! Runs the full pipeline over a directory of page images and writes a stable
//! JSON report of detection regions, masks, OCR text, and translations. Run it
//! on desktop and on Android, then diff the two reports to verify on-device
//! parity (M3 of `Android_build_plan.md`).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use koharu_config::Config;
use koharu_pipeline::{
    ComicLayoutYolo26sConfig, Committer, DetectionModel, Flux2KleinConfig, InpaintingModel,
    KoharuLayoutRFDetrSeg2XLConfig, OcrModel, Operation, Pipeline, PipelineConfig,
    PpDocLayoutV3Config, Progress, Request, RoremMixedConfig, Scope, StageOutput,
    TranslationConfig,
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, DetectionAnalysis, EntityId, Geometry, OcrAnalysis,
    PageDraft, Region, Session, Snapshot, SourceText, Translation,
};
use koharu_translator::{GenerationConfig, Language, ModelSelection, Provider, ProvidersConfig};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(version, about = "Compare Koharu pipeline output across devices")]
struct Arguments {
    /// Directory of page images, processed in file-name order.
    #[arg(short, long, value_name = "DIRECTORY")]
    input: PathBuf,

    /// Where to write the JSON report.
    #[arg(short, long, value_name = "JSON")]
    output: PathBuf,

    #[arg(long, default_value = "koharu-layout-rfdetr-seg-2xl")]
    detection: String,

    #[arg(long, default_value = "paddleocr-vl-1.6")]
    ocr: String,

    #[arg(long, default_value = "lama")]
    inpainting: String,

    #[arg(long, default_value = "en-US")]
    target_language: Language,

    #[arg(long)]
    translation_instructions: Option<String>,

    #[arg(long, default_value = "gemma4-12b-it")]
    llm: String,

    #[arg(long)]
    cpu: bool,
}

struct SessionCommitter<'a>(&'a mut Session);

#[async_trait::async_trait]
impl Committer for SessionCommitter<'_> {
    async fn commit(&mut self, output: StageOutput) -> Result<koharu_scene::Snapshot> {
        Ok(self.0.commit(output.patch).await?.snapshot)
    }
}

impl Arguments {
    fn pipeline_config(&self) -> Result<PipelineConfig> {
        Ok(PipelineConfig {
            detection: detection_model(&self.detection)?,
            ocr: ocr_model(&self.ocr)?,
            translation: TranslationConfig {
                model: ModelSelection {
                    provider: Provider::Local,
                    model: Some(self.llm.clone()),
                    quantization: None,
                    vision: true,
                    reasoning: true,
                },
                // Greedy decoding keeps the report reproducible; it is diffed
                // across devices.
                generation: GenerationConfig {
                    temperature: Some(0.0),
                    top_k: Some(1),
                    ..GenerationConfig::default()
                },
                target_language: self.target_language,
                instructions: self.translation_instructions.clone(),
            },
            inpainting: inpainting_model(&self.inpainting)?,
            processor: Default::default(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    initialize_with_retry().await;

    let mut images = image_files(&arguments.input)?;
    images.sort();
    if images.is_empty() {
        bail!("no images found in {}", arguments.input.display());
    }

    let mut session = Session::memory().await?;
    let mut pages = Vec::new();
    for path in &images {
        let source =
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let decoded = image::load_from_memory(&source).context("failed to decode input image")?;
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("page")
            .to_owned();
        let mut page = None;
        let patch = session.snapshot().patch(|edit| {
            let id = edit.add_page(
                PageDraft::new(
                    label.clone(),
                    f64::from(decoded.width()),
                    f64::from(decoded.height()),
                ),
                At::End,
            )?;
            edit.set_asset(
                id,
                &AssetRole::new("source")?,
                AssetInput::new(
                    Arc::<[u8]>::from(source.as_slice()),
                    image_media_type(path),
                    AssetMetadata {
                        width: Some(decoded.width()),
                        height: Some(decoded.height()),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            page = Some(id);
            Ok(())
        })?;
        session.commit(patch).await?;
        pages.push(page.expect("page ID is assigned by the edit"));
    }

    let device = koharu_ml::device(arguments.cpu);
    let pipeline = Pipeline::from_config(
        Config::memory(arguments.pipeline_config()?),
        Config::memory(ProvidersConfig::default()),
        device,
    )?;
    let snapshot = session.snapshot();
    let mut committer = SessionCommitter(&mut session);
    let report = pipeline
        .execute(
            snapshot,
            Request {
                operation: Operation::Full,
                scope: Scope::Pages(pages),
                progress: Some(Arc::new(|event| {
                    if let Progress::Finished { stage, elapsed, .. } = event {
                        eprintln!("{stage} finished in {:.2}s", elapsed.as_secs_f64());
                    }
                })),
                ..Request::default()
            },
            &mut committer,
        )
        .await?;
    eprintln!("pipeline finished in {:.2}s", report.elapsed.as_secs_f64());

    let document = report_document(&session.snapshot()).await?;
    let serialized = serde_json::to_string_pretty(&document)?;
    fs::write(&arguments.output, serialized)
        .with_context(|| format!("failed to write {}", arguments.output.display()))?;
    eprintln!("wrote {}", arguments.output.display());
    Ok(())
}

async fn report_document(snapshot: &Snapshot) -> Result<Value> {
    let source_role = AssetRole::new("source")?;
    let text_mask_role = AssetRole::new("text-mask")?;
    let mut pages = Vec::new();
    for page in snapshot.pages() {
        let page_id = page.id();
        let page_component = page.page()?;
        let mut regions = Vec::new();
        let mut texts = Vec::new();
        for entity in snapshot.descendants(page_id)? {
            let id = entity.id();
            if let Some(region) = entity.component::<Region>()? {
                let geometry = entity.component::<Geometry>()?;
                let detection = entity.component::<DetectionAnalysis>()?;
                regions.push(json!({
                    "id": id.to_string(),
                    "kind": region.kind.as_str(),
                    "label": region.label,
                    "bounds": geometry.as_ref().map(bounds),
                    "confidence": detection
                        .as_ref()
                        .and_then(|analysis| analysis.labels.first())
                        .map(|label| label.confidence),
                }));
            }
            if let Some(text) = entity.component::<SourceText>()? {
                let translation = entity.component::<Translation>()?;
                let ocr = entity.component::<OcrAnalysis>()?;
                texts.push(json!({
                    "id": id.to_string(),
                    "source": text.text.value,
                    "source_language": text.language.as_ref().map(|language| language.as_str()),
                    "translation": translation.as_ref().map(|value| value.text.value.clone()),
                    "direction": ocr.as_ref().map(|analysis| format!("{:?}", analysis.direction)),
                }));
            }
        }
        pages.push(json!({
            "id": page_id.to_string(),
            "label": page_component.label,
            "width": page_component.width,
            "height": page_component.height,
            "regions": regions,
            "texts": texts,
            "assets": {
                "source": asset_summary(snapshot, page_id, &source_role).await?,
                "text-mask": asset_summary(snapshot, page_id, &text_mask_role).await?,
            },
        }));
    }
    Ok(json!({ "pages": pages }))
}

async fn asset_summary(snapshot: &Snapshot, entity: EntityId, role: &AssetRole) -> Result<Value> {
    let Some(asset) = snapshot.asset(entity, role)? else {
        return Ok(Value::Null);
    };
    let bytes = snapshot.read_blob(asset.blob).await?;
    Ok(json!({
        "media_type": asset.media_type,
        "width": asset.metadata.width,
        "height": asset.metadata.height,
        "bytes": bytes.len(),
        "digest": fnv1a(&bytes),
    }))
}

fn bounds(geometry: &Geometry) -> [f64; 4] {
    if geometry.points.is_empty() {
        return [0.0; 4];
    }
    geometry.points.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[left, top, right, bottom], point| {
            [
                left.min(point.x),
                top.min(point.y),
                right.max(point.x),
                bottom.max(point.y),
            ]
        },
    )
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn detection_model(name: &str) -> Result<DetectionModel> {
    Ok(match name {
        "koharu-layout-rfdetr-seg-2xl" => {
            DetectionModel::KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig::default())
        }
        "pp-doclayout-v3" => DetectionModel::PpDocLayoutV3(PpDocLayoutV3Config::default()),
        "comic-layout-yolo26s" => {
            DetectionModel::ComicLayoutYolo26s(ComicLayoutYolo26sConfig::default())
        }
        other => bail!("unsupported detection model {other}"),
    })
}

fn ocr_model(name: &str) -> Result<OcrModel> {
    Ok(match name {
        "paddleocr-vl-1.6" => OcrModel::PaddleOcrVl1_6,
        "manga-ocr" => OcrModel::MangaOcr,
        "baberu-ocr" => OcrModel::BaberuOcr,
        "hayai-ocr" => OcrModel::HayaiOcr,
        other => bail!("unsupported OCR model {other}"),
    })
}

fn inpainting_model(name: &str) -> Result<InpaintingModel> {
    Ok(match name {
        "lama" => InpaintingModel::LaMa {},
        "aot-inpainting" => InpaintingModel::AotInpainting {},
        "flux2-klein" => InpaintingModel::Flux2Klein(Flux2KleinConfig::default()),
        "rorem-mixed" => InpaintingModel::RoremMixed(RoremMixedConfig::default()),
        other => bail!("unsupported inpainting model {other}"),
    })
}

fn image_files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if path.is_file() && is_image(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp")
    )
}

fn image_media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    }
}

async fn initialize_with_retry() {
    let mut delay = Duration::from_secs(1);
    let mut attempt = 0_u64;
    loop {
        attempt += 1;
        match koharu_ml::init().await {
            Ok(()) => return,
            Err(error) => {
                let jitter = Duration::from_millis((attempt.wrapping_mul(137)) % 251);
                let wait = delay + jitter;
                eprintln!(
                    "runtime initialization attempt {attempt} failed: {error}; retrying in {:.1}s",
                    wait.as_secs_f64()
                );
                tokio::time::sleep(wait).await;
                delay = delay.saturating_mul(2).min(Duration::from_secs(30));
            }
        }
    }
}
