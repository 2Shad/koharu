//! Translation through local and hosted providers.

mod backend;
mod error;
mod language;
mod local;
mod model;
mod prompt;
mod provider;
mod remote;

use std::{sync::Arc, time::Duration};

use koharu_ml::Device;

use error::{Error, Result};
use local::LocalTranslator;

pub use backend::{TranslationContext, TranslationRequest};
pub use language::Language;
pub use model::{GenerationConfig, Model, ModelSelection, Quantization};
pub(crate) use model::{ModelGeneration, QuantizationDefinition, display_name};
pub use provider::{Provider, ProviderConfig, ProvidersConfig};

#[derive(Clone)]
pub struct Translator {
    providers: koharu_config::Config<ProvidersConfig>,
    local: Arc<tokio::sync::Mutex<Option<LoadedLocal>>>,
    client: reqwest::Client,
    device: Device,
}

struct LoadedLocal {
    model: Option<String>,
    quantization: Option<String>,
    /// Whether the page-image projector was loaded alongside the weights. The
    /// projector is ~1 GB, so it is only held when translation will use it.
    vision: bool,
    translator: Arc<LocalTranslator>,
}

impl LoadedLocal {
    fn matches(&self, selection: &ModelSelection, vision: bool) -> bool {
        self.model == selection.model
            && self.quantization == selection.quantization
            && self.vision == vision
    }
}

impl Translator {
    pub fn from_config(
        device: Device,
        providers: koharu_config::Config<ProvidersConfig>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            providers,
            local: Arc::new(tokio::sync::Mutex::new(None)),
            client: koharu_runtime::http_client()?,
            device,
        })
    }

    #[must_use]
    pub fn model(selection: &ModelSelection) -> &'static str {
        selection.provider.into()
    }

    #[must_use]
    pub fn supports_vision(selection: &ModelSelection, generation: &GenerationConfig) -> bool {
        generation.vision.unwrap_or(false)
            && (selection.provider != Provider::Local || local::supports_vision(selection))
    }

    #[must_use]
    pub fn loaded(&self, selection: &ModelSelection, generation: &GenerationConfig) -> bool {
        if selection.provider != Provider::Local {
            return true;
        }
        let vision = Self::supports_vision(selection, generation);
        self.local
            .try_lock()
            .map(|loaded| {
                loaded
                    .as_ref()
                    .is_some_and(|loaded| loaded.matches(selection, vision))
            })
            .unwrap_or(true)
    }

    pub fn unload(&self) -> bool {
        self.local
            .try_lock()
            .map(|mut loaded| loaded.take().is_some())
            .unwrap_or(false)
    }

    #[tracing::instrument(skip_all)]
    pub async fn load_model(
        &self,
        selection: &ModelSelection,
        generation: &GenerationConfig,
    ) -> anyhow::Result<()> {
        if selection.provider == Provider::Local {
            self.local(selection, Self::supports_vision(selection, generation))
                .await?;
        }
        Ok(())
    }

    pub async fn translate(
        &self,
        selection: &ModelSelection,
        generation: GenerationConfig,
        mut request: TranslationRequest,
    ) -> anyhow::Result<(&'static str, Vec<String>)> {
        let provider = selection.provider;
        let provider_id: &'static str = provider.into();
        if request.segments.is_empty() {
            return Ok((provider_id, request.segments));
        }

        let generation = generation.for_model(selection);

        let vision = Self::supports_vision(selection, &generation);

        if vision {
            request.prepare_image()?;
        } else {
            request.remove_image();
        }

        let expected = request.segments.len();
        let translated = if provider == Provider::Local {
            self.local(selection, vision)
                .await?
                .translate(request, generation)
                .await?
        } else {
            let providers = self.providers.read()?.clone();
            remote::translate(&self.client, &providers, selection, &generation, &request).await?
        };
        if translated.len() != expected {
            return Err(Error::SegmentCount {
                provider: provider_id,
                expected,
                actual: translated.len(),
            }
            .into());
        }
        Ok((provider_id, translated))
    }

    #[tracing::instrument(skip_all)]
    pub async fn models() -> anyhow::Result<Vec<Model>> {
        static CLIENT: tokio::sync::OnceCell<reqwest::Client> = tokio::sync::OnceCell::const_new();

        let providers = ProvidersConfig::load()?;
        let providers = providers.read()?.clone();
        let client = CLIENT
            .get_or_try_init(|| async {
                koharu_runtime::configure_reqwest_tls(reqwest::Client::builder())
                    .timeout(Duration::from_secs(5))
                    .build()
            })
            .await?;
        let mut models = local::models();
        models.extend(remote::models(client, &providers).await);
        Ok(models)
    }

    async fn local(
        &self,
        selection: &ModelSelection,
        vision: bool,
    ) -> Result<Arc<LocalTranslator>> {
        let mut loaded = self.local.lock().await;
        if loaded
            .as_ref()
            .is_none_or(|loaded| !loaded.matches(selection, vision))
        {
            *loaded = Some(LoadedLocal {
                model: selection.model.clone(),
                quantization: selection.quantization.clone(),
                vision,
                translator: Arc::new(
                    LocalTranslator::load(self.device.clone(), selection, vision).await?,
                ),
            });
        }
        Ok(Arc::clone(
            &loaded
                .as_ref()
                .expect("local translator was loaded")
                .translator,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_selection(model: &str) -> ModelSelection {
        ModelSelection {
            provider: Provider::Local,
            model: Some(model.to_owned()),
            quantization: None,
            vision: true,
            reasoning: true,
        }
    }

    #[test]
    fn local_vision_requires_capability_and_generation_setting() {
        assert!(Translator::supports_vision(
            &local_selection("gemma4-e2b-it"),
            &GenerationConfig {
                vision: Some(true),
                ..GenerationConfig::default()
            }
        ));
        assert!(!Translator::supports_vision(
            &local_selection("gemma4-e2b-it"),
            &GenerationConfig {
                vision: Some(false),
                ..GenerationConfig::default()
            }
        ));
        assert!(!Translator::supports_vision(
            &local_selection("lfm2.5-1.2b-instruct"),
            &GenerationConfig {
                vision: Some(true),
                ..GenerationConfig::default()
            }
        ));
    }
}
