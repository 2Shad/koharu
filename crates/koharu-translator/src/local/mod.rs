use std::sync::Arc;

use anyhow::Context;
use koharu_ml::llm::{
    ChatMessage, ChatTemplateOptions, Input, Llm, LoadOptions, MtmdOptions, media_marker,
};

mod catalog;

pub use catalog::LocalConfig;
use catalog::LocalModelDescriptor;
pub(crate) use catalog::{DEFAULT_MODEL, DEFAULT_QUANTIZATION};

use crate::{
    Device, GenerationConfig, Language, Model, ModelSelection, Prompt, Provider, Quantization,
    Result,
};

#[derive(Debug)]
pub struct LocalTranslator {
    descriptor: LocalModelDescriptor,
    llm: Arc<Llm>,
}

impl LocalTranslator {
    pub async fn load(device: Device, selection: &ModelSelection) -> Result<Self> {
        let model = selection
            .model
            .as_deref()
            .context("local translation requires a selected model")?;
        let descriptor = catalog::MODELS
            .iter()
            .copied()
            .find(|descriptor| descriptor.id == model)
            .with_context(|| format!("unknown local translator '{model}'"))?;
        let resolved = descriptor.resolve(selection).await?;
        let options = LoadOptions {
            mtmd: resolved.projector.map(MtmdOptions::new),
            ..LoadOptions::default()
        };
        let llm = Llm::load_with_options(device, resolved.model, options)
            .await
            .context("failed to load local translation model")?;
        if llm.capabilities().vision != descriptor.projector.is_some() {
            return Err(anyhow::anyhow!(
                "local translator vision capability does not match its catalog"
            )
            .into());
        }
        Ok(Self {
            descriptor,
            llm: Arc::new(llm),
        })
    }

    pub(crate) fn supports_target_language(&self, language: Language) -> bool {
        self.descriptor.target_languages.contains(language)
    }

    pub(crate) async fn complete(
        &self,
        prompt: Prompt,
        generation: GenerationConfig,
    ) -> Result<String> {
        let Prompt {
            system,
            user,
            schema,
            image,
            ..
        } = prompt;
        let rendered = self.render_prompt(
            system,
            user,
            image.is_some(),
            generation.reasoning.unwrap_or(false) && self.descriptor.reasoning,
        )?;
        let llm = Arc::clone(&self.llm);
        let generation = self.descriptor.generation.options(generation);
        let output = tokio_rayon::spawn(move || {
            let input = image.as_deref().map_or_else(
                || Input::new(&rendered),
                |image| Input::new(&rendered).with_image(image),
            );
            llm.inference_with_json_schema(&input, &generation, &schema)
        })
        .await?;
        Ok(output.text)
    }

    fn render_prompt(
        &self,
        system: String,
        user: String,
        image: bool,
        reasoning: bool,
    ) -> Result<String> {
        let user = if image {
            format!("{}\n{user}", media_marker())
        } else {
            user
        };
        Ok(self
            .llm
            .render_chat_prompt_with_options(
                &[ChatMessage::system(system), ChatMessage::user(user)],
                ChatTemplateOptions {
                    add_generation_prompt: true,
                    enable_thinking: reasoning,
                },
            )
            .context("failed to render local prompt")?)
    }
}

pub(crate) fn models() -> Vec<Model> {
    catalog::MODELS
        .iter()
        .map(|descriptor| Model {
            provider: Provider::Local,
            model: Some(descriptor.id.to_owned()),
            name: descriptor.name.to_owned(),
            quantizations: descriptor
                .quantizations
                .iter()
                .map(|quantization| Quantization {
                    id: quantization.id.to_owned(),
                    name: quantization.name.to_owned(),
                    downloaded: std::iter::once(quantization.filename)
                        .chain(descriptor.projector)
                        .all(|filename| descriptor.file(filename).exists()),
                })
                .collect(),
            vision: descriptor.projector.is_some(),
            reasoning: descriptor.reasoning,
        })
        .collect()
}

pub(crate) fn supports_vision(selection: &ModelSelection) -> bool {
    selection.model.as_deref().is_some_and(|model| {
        catalog::MODELS
            .iter()
            .find(|descriptor| descriptor.id == model)
            .is_some_and(|descriptor| descriptor.projector.is_some())
    })
}
