//! Drama 3 dialogue: [`direction`] adds cues and speakers to translations;
//! [`Speech`] voices scripts through a managed audio.cpp server
//! (`fish_drama3`).

pub mod direction;
pub mod script;

mod config;
mod model;
mod server;
mod wav;

use std::sync::Arc;

use anyhow::{Result, bail};
use serde_json::json;

pub use config::{CharacterVoice, SpeechConfig};
pub use model::SpeechModel;
pub use wav::WavFormat;

use server::{Server, ServerSettings};

/// Upstream port's default seed, validated on its regression prompts.
const DEFAULT_SEED: u64 = 123;

#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    pub script: &'a str,
    pub character: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Audio {
    pub wav: Vec<u8>,
    pub format: WavFormat,
}

#[derive(Clone)]
pub struct Speech {
    config: koharu_config::Config<SpeechConfig>,
    client: reqwest::Client,
    server: Arc<tokio::sync::Mutex<Option<Server>>>,
}

impl Speech {
    pub fn from_config(config: koharu_config::Config<SpeechConfig>) -> Result<Self> {
        Ok(Self {
            config,
            // Loopback only; system proxies must not see these requests.
            client: reqwest::Client::builder().no_proxy().build()?,
            server: Arc::default(),
        })
    }

    pub fn model(&self) -> Result<SpeechModel> {
        let config = self.config.read()?;
        Ok(SpeechModel::status(&config))
    }

    /// Downloads the model and server unless settings name local copies.
    pub async fn install(&self) -> Result<()> {
        let config = self.config.read()?.clone();
        // The server install fails fast without CUDA, before the model download.
        if config.server.is_none() {
            koharu_runtime::speech_server().await?;
        }
        if config.model.is_none() {
            model::download().await?;
        }
        Ok(())
    }

    pub async fn synthesize(&self, line: Line<'_>) -> Result<Audio> {
        let input = script::sanitize(line.script);
        if script::cues(&input).is_empty() {
            bail!("dialogue line has nothing to voice");
        }
        let config = self.config.read()?.clone();
        let endpoint = self.endpoint(&config).await?;

        let character = line
            .character
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let mut body = json!({
            "model": server::MODEL_ID,
            "input": input,
            "seed": character.map_or(DEFAULT_SEED, seed).to_string(),
            "max_tokens": config.max_tokens,
        });
        if let Some(voice) = character.and_then(|character| config.voice(character)) {
            let reference = std::path::absolute(&voice.reference_audio)?;
            body["voice_ref"] = json!({ "type": "path", "path": reference });
            body["reference_text"] = json!(voice.reference_text);
        }

        let response = match self
            .client
            .post(format!("{endpoint}/v1/audio/speech"))
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return Err(self.failure(error.into(), "speech request failed").await),
        };
        let status = response.status();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(self
                    .failure(error.into(), "failed to read speech response")
                    .await);
            }
        };
        if !status.is_success() {
            bail!(
                "speech server returned {status}: {}",
                String::from_utf8_lossy(&bytes).trim()
            );
        }
        let wav = bytes.to_vec();
        let format = wav::format(&wav)?;
        Ok(Audio { wav, format })
    }

    /// Adds the exit report when a server crash caused `error`.
    async fn failure(&self, error: anyhow::Error, context: &'static str) -> anyhow::Error {
        let mut server = self.server.lock().await;
        match server.as_mut().and_then(Server::exit_report) {
            Some(report) => {
                *server = None;
                error.context(format!(
                    "speech server {report}\nIf it ran out of GPU memory, raise CPU expert layers in Settings > Speech."
                ))
            }
            None => error.context(context),
        }
    }

    pub async fn unload(&self) {
        self.server.lock().await.take();
    }

    async fn endpoint(&self, config: &SpeechConfig) -> Result<String> {
        let settings = ServerSettings::resolve(config).await?;
        let mut server = self.server.lock().await;
        let stale = server
            .as_mut()
            .is_none_or(|running| running.exited() || running.settings() != &settings);
        if stale {
            // Kill first; with `lazy_load` the replacement loads nothing until a request.
            *server = None;
            *server = Some(Server::start(settings, &self.client).await?);
        }
        Ok(server
            .as_ref()
            .expect("speech server was started")
            .endpoint()
            .to_owned())
    }
}

/// FNV-1a of the lowercased name, folded to u32: audio.cpp reads seeds with
/// `std::stoul`, which is 32-bit on Windows.
fn seed(character: &str) -> u64 {
    let hash = character
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    (hash ^ (hash >> 32)) & u64::from(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_seeds_are_stable_and_case_insensitive() {
        assert_eq!(seed("Mika"), seed("mika"));
        assert_ne!(seed("Mika"), seed("Ren"));
        assert!(seed("Ayane") <= u64::from(u32::MAX));
    }

    #[tokio::test]
    async fn synthesize_requires_the_voice_model() {
        let speech = Speech::from_config(koharu_config::Config::memory(SpeechConfig {
            model: Some("missing-drama-3.gguf".into()),
            ..SpeechConfig::default()
        }))
        .unwrap();
        let model = speech.model().unwrap();
        assert!(!model.ready && model.local);
        let error = speech
            .synthesize(Line {
                script: "[sighs] Fine.",
                character: None,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn synthesize_rejects_empty_lines() {
        let speech =
            Speech::from_config(koharu_config::Config::memory(SpeechConfig::default())).unwrap();
        let error = speech
            .synthesize(Line {
                script: " <|speaker:0|> ",
                character: None,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("nothing to voice"));
    }
}
