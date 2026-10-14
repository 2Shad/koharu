use anyhow::Result;
use async_trait::async_trait;
use koharu_scene::{Authored, Dialogue, Origin, Translation};
use koharu_speech::direction::{self, DirectionLine, DirectionRequest};
use koharu_translator::{GenerationConfig, ModelSelection, Prompt, Translator};

use crate::{DialogueConfig, TranslationConfig};

use super::{StageInput, StageProcessor, finish, generation};

const PRODUCER: &str = "dev.koharu.pipeline.dialogue";

pub(super) struct Processor {
    model: ModelSelection,
    generation: GenerationConfig,
    language: String,
    instructions: Option<String>,
    translator: Translator,
}

impl Processor {
    pub(super) fn new(
        config: DialogueConfig,
        translation: &TranslationConfig,
        translator: Translator,
    ) -> Self {
        Self {
            model: config.model.unwrap_or_else(|| translation.model.clone()),
            generation: config.generation.unwrap_or(translation.generation),
            language: translation.target_language.to_string(),
            instructions: config.instructions,
            translator,
        }
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> &'static str {
        Translator::model(&self.model)
    }

    fn unload(&self) -> bool {
        self.translator.unload()
    }

    // Machine-translation providers cannot direct; their lines keep the
    // translation as an uncued script.
    fn skip(&self, _input: &StageInput) -> Result<bool> {
        Ok(!Translator::supports_completion(&self.model))
    }

    async fn load(&self) -> Result<()> {
        self.translator.load_model(&self.model).await
    }

    async fn process(&self, input: StageInput) -> Result<koharu_scene::Patch> {
        let mut targets = Vec::new();
        if let Some(group) = input.scene.page(input.page)?.text_group()? {
            for layer in group.text_layers()? {
                if !input.contains_entity(layer.id())? {
                    continue;
                }
                let content = layer.content()?;
                let (Some(source), Some(translation)) = (content.source()?, content.translation()?)
                else {
                    continue;
                };
                if translation.text.value.trim().is_empty()
                    || content
                        .dialogue()?
                        .is_some_and(|dialogue| matches!(dialogue.script.origin, Origin::User))
                {
                    continue;
                }
                targets.push((
                    content.id(),
                    DirectionLine {
                        source: source.text.value,
                        translation: translation.text.value,
                    },
                ));
            }
        }
        if targets.is_empty() {
            return finish(input.scene.edit());
        }

        let lines: Vec<_> = targets.iter().map(|(_, line)| line.clone()).collect();
        let request = DirectionRequest {
            lines,
            language: self.language.clone(),
            instructions: self.instructions.clone(),
        };
        let prompt = direction::prompt(&request)?;
        let image = if Translator::supports_vision(&self.model, &self.generation) {
            input.images.get(&input.scene, input.page, "source").await?
        } else {
            None
        };
        let (provider, text) = self
            .translator
            .complete(
                &self.model,
                self.generation,
                Prompt {
                    name: prompt.name,
                    system: prompt.system,
                    user: prompt.user,
                    schema: prompt.schema,
                    image,
                },
            )
            .await?;
        let directions = direction::parse(&text, &request.lines)?;

        let generated = generation(PRODUCER, provider)?;
        let mut edit = input.scene.edit_as(generated.clone());
        for (entity, _) in &targets {
            edit.observe::<Translation>(*entity)?;
            edit.observe::<Dialogue>(*entity)?;
        }
        for ((entity, _), direction) in targets.into_iter().zip(directions) {
            edit.set(
                entity,
                &Dialogue {
                    script: Authored::generated(direction.script, generated.clone()),
                    character: direction.character,
                },
            )?;
        }
        finish(edit)
    }
}
