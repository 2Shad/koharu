use std::path::PathBuf;

use anyhow::{Result, bail};
use koharu_runtime::HuggingFaceFile;
use serde::Serialize;
use specta::Type;

use crate::SpeechConfig;

/// Q8_0 backbone, Q2_K experts: smallest `fish_drama3` package; fits 12 GB
/// GPUs with expert offload.
const MODEL: HuggingFaceFile<'static> = HuggingFaceFile::pinned(
    "Jmica/Drama-3-GGUF",
    "3cc54482e2166e9d2d45e562e52ea3a810f39fe3",
    "drama-3-backbone-8bit-experts-Q2_K.gguf",
);
const MODEL_BYTES: u64 = 17_053_639_872;

/// Whether the voice model can be used without a download.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
pub struct SpeechModel {
    pub ready: bool,
    /// `SpeechConfig::model` is set.
    pub local: bool,
    #[specta(type = f64)]
    pub download_bytes: u64,
}

impl SpeechModel {
    pub(crate) fn status(config: &SpeechConfig) -> Self {
        Self {
            ready: config
                .model
                .as_ref()
                .map_or_else(|| MODEL.exists(), |path| path.is_file()),
            local: config.model.is_some(),
            download_bytes: MODEL_BYTES,
        }
    }
}

/// Never downloads; the 17 GB package is fetched only by [`download`].
pub(crate) fn path(config: &SpeechConfig) -> Result<PathBuf> {
    match &config.model {
        Some(path) if path.is_file() => Ok(path.clone()),
        Some(path) => bail!("Drama 3 model {} does not exist", path.display()),
        None => match MODEL.cached() {
            Some(path) => Ok(path),
            None => {
                bail!("download the Drama 3 voice model in Settings > Speech before voicing lines")
            }
        },
    }
}

pub(crate) async fn download() -> Result<PathBuf> {
    MODEL.resolve().await
}
