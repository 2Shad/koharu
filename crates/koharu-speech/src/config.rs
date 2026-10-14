use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

/// `[speech]` config section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct SpeechConfig {
    /// Local `audiocpp_server`; replaces the managed download.
    pub server: Option<PathBuf>,
    /// Local Drama 3 GGUF; replaces the managed download.
    pub model: Option<PathBuf>,
    /// Routed-expert layers kept in host RAM; unset picks from VRAM.
    pub cpu_expert_layers: Option<u32>,
    pub context_length: u32,
    /// Max acoustic frames per line (~21.5/s).
    pub max_tokens: u32,
    pub threads: u32,
    pub voices: Vec<CharacterVoice>,
}

impl Default for SpeechConfig {
    fn default() -> Self {
        Self {
            server: None,
            model: None,
            cpu_expert_layers: None,
            context_length: 2048,
            max_tokens: 500,
            threads: 8,
            voices: Vec::new(),
        }
    }
}

impl SpeechConfig {
    pub fn load() -> anyhow::Result<koharu_config::Config<Self>> {
        koharu_config::load("speech")
    }

    pub(crate) fn voice(&self, character: &str) -> Option<&CharacterVoice> {
        let character = character.trim().to_lowercase();
        self.voices
            .iter()
            .find(|voice| voice.character.trim().to_lowercase() == character)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct CharacterVoice {
    pub character: String,
    /// WAV, ideally 5-15 s of clean speech.
    pub reference_audio: PathBuf,
    /// Exact transcript of `reference_audio`.
    pub reference_text: String,
}
