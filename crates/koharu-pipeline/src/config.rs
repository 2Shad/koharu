use anyhow::{Result, bail};
use koharu_translator::{GenerationConfig, Language};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use specta::Type;

use crate::stages::{
    ComicLayoutYolo26sConfig, Flux2KleinConfig, KoharuLayoutRFDetrSeg2XLConfig,
    PpDocLayoutV3Config, RoremMixedConfig,
};

#[derive(Clone, Debug, PartialEq, Type)]
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub ocr: OcrModel,
    pub translation: TranslationConfig,
    pub inpainting: InpaintingModel,
    /// Settings for every model are kept independently of the active model.
    /// The active stage fields above only select which profile is used.
    pub processor: ProcessorConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct PipelineFile {
    detection: ModelSelection,
    ocr: ModelSelection,
    translation: TranslationConfig,
    inpainting: ModelSelection,
    #[serde(default)]
    processor: ProcessorConfig,
}

impl Default for PipelineFile {
    fn default() -> Self {
        Self {
            detection: ModelSelection {
                model: DetectionModel::default_for_platform().name().to_owned(),
            },
            ocr: ModelSelection {
                model: OcrModel::default_for_platform().name().to_owned(),
            },
            translation: TranslationConfig::default(),
            inpainting: ModelSelection {
                model: InpaintingModel::default_for_platform().name().to_owned(),
            },
            processor: ProcessorConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct ModelSelection {
    model: String,
}

impl Serialize for PipelineConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let detection = self.detection.name();
        let ocr = self.ocr.name();
        let inpainting = self.inpainting.name();
        let mut processor = self.processor.clone();
        match &self.detection {
            DetectionModel::KoharuLayoutRFDetrSeg2XL(config) => {
                processor
                    .koharu_layout_rfdetr_seg_2xl
                    .get_or_insert_with(|| config.clone());
            }
            DetectionModel::PpDocLayoutV3(config) => {
                processor
                    .pp_doclayout_v3
                    .get_or_insert_with(|| config.clone());
            }
            DetectionModel::ComicLayoutYolo26s(config) => {
                processor
                    .comic_layout_yolo26s
                    .get_or_insert_with(|| config.clone());
            }
        }
        match &self.inpainting {
            InpaintingModel::Flux2Klein(config) => {
                processor.flux2_klein.get_or_insert_with(|| config.clone());
            }
            InpaintingModel::RoremMixed(config) => {
                processor.rorem_mixed.get_or_insert_with(|| config.clone());
            }
            InpaintingModel::LaMa {} | InpaintingModel::AotInpainting {} => {}
        }
        PipelineFile {
            detection: ModelSelection {
                model: detection.to_owned(),
            },
            ocr: ModelSelection {
                model: ocr.to_owned(),
            },
            translation: self.translation.clone(),
            inpainting: ModelSelection {
                model: inpainting.to_owned(),
            },
            processor,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PipelineConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let file = PipelineFile::deserialize(deserializer)?;
        let detection = match file.detection.model.as_str() {
            "koharu-layout-rfdetr-seg-2xl" => DetectionModel::KoharuLayoutRFDetrSeg2XL(
                file.processor
                    .koharu_layout_rfdetr_seg_2xl
                    .clone()
                    .unwrap_or_default(),
            ),
            "pp-doclayout-v3" => DetectionModel::PpDocLayoutV3(
                file.processor.pp_doclayout_v3.clone().unwrap_or_default(),
            ),
            "comic-layout-yolo26s" => DetectionModel::ComicLayoutYolo26s(
                file.processor
                    .comic_layout_yolo26s
                    .clone()
                    .unwrap_or_default(),
            ),
            model => {
                return Err(serde::de::Error::custom(format!(
                    "unsupported detection model {model}"
                )));
            }
        };
        let ocr = match file.ocr.model.as_str() {
            "paddleocr-vl-1.6" => OcrModel::PaddleOcrVl1_6,
            "manga-ocr" => OcrModel::MangaOcr,
            "baberu-ocr" => OcrModel::BaberuOcr,
            "hayai-ocr" => OcrModel::HayaiOcr,
            model => {
                return Err(serde::de::Error::custom(format!(
                    "unsupported OCR model {model}"
                )));
            }
        };
        let inpainting = match file.inpainting.model.as_str() {
            "lama" => InpaintingModel::LaMa {},
            "aot-inpainting" => InpaintingModel::AotInpainting {},
            "flux2-klein" => {
                InpaintingModel::Flux2Klein(file.processor.flux2_klein.clone().unwrap_or_default())
            }
            "rorem-mixed" => {
                InpaintingModel::RoremMixed(file.processor.rorem_mixed.clone().unwrap_or_default())
            }
            model => {
                return Err(serde::de::Error::custom(format!(
                    "unsupported inpainting model {model}"
                )));
            }
        };
        Ok(Self {
            detection,
            ocr,
            translation: file.translation,
            inpainting,
            processor: file.processor,
        })
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            detection: DetectionModel::default_for_platform(),
            ocr: OcrModel::default_for_platform(),
            translation: TranslationConfig::default(),
            inpainting: InpaintingModel::default_for_platform(),
            processor: ProcessorConfig::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TranslationConfig {
    pub model: koharu_translator::ModelSelection,
    pub generation: GenerationConfig,
    #[specta(type = String)]
    pub target_language: Language,
    pub instructions: Option<String>,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            model: default_translation_model(),
            generation: GenerationConfig::default(),
            target_language: Language::English,
            instructions: None,
        }
    }
}

/// Picks a local model that fits the device's mobile RAM class.
///
/// The desktop default (Gemma 4 12B Q4) is larger than every Android resident
/// budget, so a first launch on a phone has to start from the profile table
/// instead of accepting a model the loader will refuse.
fn default_translation_model() -> koharu_translator::ModelSelection {
    if cfg!(target_os = "android") {
        mobile_translation_model(
            koharu_ml::llm::RamClass::for_device().unwrap_or(koharu_ml::llm::RamClass::Low),
        )
    } else {
        koharu_translator::ModelSelection::default()
    }
}

fn mobile_translation_model(ram: koharu_ml::llm::RamClass) -> koharu_translator::ModelSelection {
    use koharu_ml::llm::RamClass;
    use koharu_translator::{ModelSelection, Provider};

    let (model, quantization) = match ram {
        RamClass::Low => ("gemma4-e2b-it", "Q2_K_XL"),
        RamClass::Mid => ("gemma4-e4b-it", "Q2_K_XL"),
        RamClass::High => ("gemma4-e4b-it", "Q4_K_XL"),
    };
    ModelSelection {
        provider: Provider::Local,
        model: Some(model.to_owned()),
        quantization: Some(quantization.to_owned()),
        vision: false,
        reasoning: false,
    }
}

impl PipelineConfig {
    pub fn load() -> anyhow::Result<koharu_config::Config<Self>> {
        koharu_config::load("pipeline")
    }

    pub fn detection(&self) -> Result<DetectionModel> {
        match &self.detection {
            DetectionModel::KoharuLayoutRFDetrSeg2XL(config) => {
                Ok(DetectionModel::KoharuLayoutRFDetrSeg2XL(
                    self.processor
                        .koharu_layout_rfdetr_seg_2xl
                        .clone()
                        .unwrap_or_else(|| config.clone()),
                ))
            }
            DetectionModel::PpDocLayoutV3(config) => Ok(DetectionModel::PpDocLayoutV3(
                self.processor
                    .pp_doclayout_v3
                    .clone()
                    .unwrap_or_else(|| config.clone()),
            )),
            DetectionModel::ComicLayoutYolo26s(config) => Ok(DetectionModel::ComicLayoutYolo26s(
                self.processor
                    .comic_layout_yolo26s
                    .clone()
                    .unwrap_or_else(|| config.clone()),
            )),
        }
    }

    pub fn inpainting(&self) -> Result<InpaintingModel> {
        match &self.inpainting {
            InpaintingModel::LaMa {} => Ok(InpaintingModel::LaMa {}),
            InpaintingModel::AotInpainting {} => Ok(InpaintingModel::AotInpainting {}),
            InpaintingModel::Flux2Klein(config) => Ok(InpaintingModel::Flux2Klein(
                self.processor
                    .flux2_klein
                    .clone()
                    .unwrap_or_else(|| config.clone()),
            )),
            InpaintingModel::RoremMixed(config) => Ok(InpaintingModel::RoremMixed(
                self.processor
                    .rorem_mixed
                    .clone()
                    .unwrap_or_else(|| config.clone()),
            )),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let _ = self.detection()?;
        let _ = self.inpainting()?;
        if !matches!(
            self.ocr,
            OcrModel::PaddleOcrVl1_6
                | OcrModel::MangaOcr
                | OcrModel::BaberuOcr
                | OcrModel::HayaiOcr
        ) {
            bail!("unsupported OCR model")
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct ProcessorConfig {
    #[serde(rename = "koharu-layout-rfdetr-seg-2xl")]
    pub koharu_layout_rfdetr_seg_2xl: Option<KoharuLayoutRFDetrSeg2XLConfig>,
    #[serde(rename = "pp-doclayout-v3")]
    pub pp_doclayout_v3: Option<PpDocLayoutV3Config>,
    #[serde(rename = "comic-layout-yolo26s")]
    pub comic_layout_yolo26s: Option<ComicLayoutYolo26sConfig>,
    #[serde(rename = "flux2-klein")]
    pub flux2_klein: Option<Flux2KleinConfig>,
    #[serde(rename = "rorem-mixed")]
    pub rorem_mixed: Option<RoremMixedConfig>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model")]
pub enum DetectionModel {
    #[serde(rename = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig),
    #[serde(rename = "pp-doclayout-v3")]
    PpDocLayoutV3(PpDocLayoutV3Config),
    #[serde(rename = "comic-layout-yolo26s")]
    ComicLayoutYolo26s(ComicLayoutYolo26sConfig),
}

impl DetectionModel {
    /// Detection runs on-device on every platform. Mobile defaults to the
    /// comic-native YOLO26s segmenter: its labels map directly onto the
    /// text/bubble/panel contract and it has the smallest footprint, unlike the
    /// document-oriented PP-DocLayout-V3 or the 1152px RF-DETR Seg 2XL.
    #[must_use]
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "android") {
            Self::ComicLayoutYolo26s(ComicLayoutYolo26sConfig::default())
        } else {
            Self::KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig::default())
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::KoharuLayoutRFDetrSeg2XL(_) => "koharu-layout-rfdetr-seg-2xl",
            Self::PpDocLayoutV3(_) => "pp-doclayout-v3",
            Self::ComicLayoutYolo26s(_) => "comic-layout-yolo26s",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model")]
pub enum OcrModel {
    #[serde(rename = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6,
    #[serde(rename = "manga-ocr")]
    MangaOcr,
    #[serde(rename = "baberu-ocr")]
    BaberuOcr,
    #[serde(rename = "hayai-ocr")]
    HayaiOcr,
}

impl OcrModel {
    /// Mobile defaults to Manga OCR: the target library is Japanese manga, its
    /// weights are small, and it shares the Torch runtime with detection and
    /// inpainting. Other scripts stay on PaddleOCR-VL, which is the desktop
    /// default.
    #[must_use]
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "android") {
            Self::MangaOcr
        } else {
            Self::PaddleOcrVl1_6
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::PaddleOcrVl1_6 => "paddleocr-vl-1.6",
            Self::MangaOcr => "manga-ocr",
            Self::BaberuOcr => "baberu-ocr",
            Self::HayaiOcr => "hayai-ocr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model")]
pub enum InpaintingModel {
    #[serde(rename = "lama")]
    LaMa {},
    #[serde(rename = "aot-inpainting")]
    AotInpainting {},
    #[serde(rename = "flux2-klein")]
    Flux2Klein(Flux2KleinConfig),
    #[serde(rename = "rorem-mixed")]
    RoremMixed(RoremMixedConfig),
}

impl InpaintingModel {
    /// Mobile defaults to AOT: it is Torch-only, tiny, and fast. LaMa stays the
    /// desktop default and is selectable on mobile.
    #[must_use]
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "android") {
            Self::AotInpainting {}
        } else {
            Self::LaMa {}
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::LaMa {} => "lama",
            Self::AotInpainting {} => "aot-inpainting",
            Self::Flux2Klein(_) => "flux2-klein",
            Self::RoremMixed(_) => "rorem-mixed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_select_one_processor_for_each_phase() {
        let config = PipelineConfig::default();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::PaddleOcrVl1_6));
        assert!(matches!(config.inpainting, InpaintingModel::LaMa {}));
    }

    #[test]
    fn mobile_translation_profiles_follow_the_ram_class() {
        use koharu_ml::llm::RamClass;

        for (ram, model, quantization) in [
            (RamClass::Low, "gemma4-e2b-it", "Q2_K_XL"),
            (RamClass::Mid, "gemma4-e4b-it", "Q2_K_XL"),
            (RamClass::High, "gemma4-e4b-it", "Q4_K_XL"),
        ] {
            let selection = mobile_translation_model(ram);
            assert_eq!(selection.model.as_deref(), Some(model));
            assert_eq!(selection.quantization.as_deref(), Some(quantization));
            assert_eq!(selection.provider, koharu_translator::Provider::Local);
            assert!(!selection.vision);
            assert!(!selection.reasoning);
        }
    }

    #[test]
    fn parses_phase_keyed_processor_configuration() {
        let config: PipelineConfig = toml::from_str(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"

                [ocr]
                model = "baberu-ocr"

                [inpainting]
                model = "rorem-mixed"

                [processor."rorem-mixed"]
                prompt = "Remove the lettering."
                negative_prompt = "letters, words"
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::BaberuOcr));
        assert!(matches!(
            config.inpainting(),
            Ok(InpaintingModel::RoremMixed(config))
                if config.prompt == "Remove the lettering."
                    && config.negative_prompt == "letters, words"
        ));
    }

    #[test]
    fn missing_slots_use_defaults() {
        let config = toml::from_str::<PipelineConfig>("").unwrap();

        assert_eq!(config, PipelineConfig::default());
    }

    #[test]
    fn ignores_unknown_model_configuration_fields() {
        let config = toml::from_str::<PipelineConfig>(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"
                legacy_threshold = 0.5

                [ocr]
                model = "paddleocr-vl-1.6"
                legacy_language = "ja"

                [inpainting]
                model = "lama"
                legacy_resolution = 1024
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::PaddleOcrVl1_6));
        assert!(matches!(config.inpainting(), Ok(InpaintingModel::LaMa {})));
    }

    #[test]
    fn parses_detection_and_generative_inpainting_options() {
        let config = toml::from_str::<PipelineConfig>(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"

                [inpainting]
                model = "flux2-klein"

                [processor."koharu-layout-rfdetr-seg-2xl"]
                text_threshold = 0.25
                bubble_threshold = 0.45
                panel_threshold = 0.55

                [processor."flux2-klein"]
                prompt = "Reconstruct the illustration without text."
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection().unwrap(),
            DetectionModel::KoharuLayoutRFDetrSeg2XL(config)
                if config.text_threshold == Some(0.25)
                    && config.bubble_threshold == Some(0.45)
                    && config.panel_threshold == Some(0.55)
        ));
        assert!(matches!(
            config.inpainting().unwrap(),
            InpaintingModel::Flux2Klein(config)
                if config.prompt == "Reconstruct the illustration without text."
        ));
    }

    #[test]
    fn keeps_profiles_separate_from_active_stage_selection() {
        let config = toml::from_str::<PipelineConfig>(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"

                [inpainting]
                model = "flux2-klein"

                [processor."flux2-klein"]
                prompt = "saved prompt"
            "#,
        )
        .unwrap();

        let InpaintingModel::Flux2Klein(config) = config.inpainting().unwrap() else {
            panic!("expected FLUX profile")
        };
        assert_eq!(config.prompt, "saved prompt");
    }

    #[test]
    fn serializes_model_profiles_under_processor() {
        let config = PipelineConfig {
            detection: DetectionModel::KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig {
                text_threshold: Some(0.25),
                ..Default::default()
            }),
            ocr: OcrModel::PaddleOcrVl1_6,
            translation: TranslationConfig::default(),
            inpainting: InpaintingModel::Flux2Klein(Flux2KleinConfig {
                prompt: "Keep the line art.".to_owned(),
            }),
            processor: ProcessorConfig::default(),
        };
        let document = toml::to_string(&config).unwrap();
        assert!(document.contains("[detection]\nmodel = \"koharu-layout-rfdetr-seg-2xl\""));
        assert!(document.contains("[processor.koharu-layout-rfdetr-seg-2xl]"));
        assert!(document.contains("[processor.flux2-klein]"));
        assert!(document.contains("[translation]"));
        assert!(!document.contains("prompt = \"Keep the line art.\"\n[inpainting]"));

        let restored = toml::from_str::<PipelineConfig>(&document).unwrap();
        assert!(matches!(
            restored.inpainting().unwrap(),
            InpaintingModel::Flux2Klein(config) if config.prompt == "Keep the line art."
        ));
    }
}
