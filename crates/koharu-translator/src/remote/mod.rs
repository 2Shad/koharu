mod caiyun;
mod claude;
mod deepl;
mod deepseek;
mod gemini;
mod google_cloud;
mod grok;
mod lm_studio;
mod minimax;
mod openai;
mod openai_compatible;
mod openrouter;

use anyhow::Context;
use futures::{FutureExt, future::BoxFuture, future::join_all};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;

pub use caiyun::CaiyunConfig;
pub use claude::ClaudeConfig;
pub use deepl::DeepLConfig;
pub use deepseek::DeepSeekConfig;
pub use gemini::GeminiConfig;
pub use google_cloud::GoogleCloudConfig;
pub use grok::GrokConfig;
pub use lm_studio::LmStudioConfig;
pub use minimax::MiniMaxConfig;
pub use openai::OpenAiConfig;
pub use openai_compatible::OpenAiCompatibleConfig;
pub use openrouter::OpenRouterConfig;

use crate::{
    Error, GenerationConfig, Model, ModelSelection, Prompt, Provider, ProvidersConfig, Result,
    TranslationRequest,
};

pub(crate) async fn complete(
    client: &Client,
    providers: &ProvidersConfig,
    selection: &ModelSelection,
    generation: &GenerationConfig,
    prompt: &Prompt,
) -> Result<String> {
    let model = || {
        selection
            .model
            .as_deref()
            .with_context(|| format!("{} requires a selected model", selection.provider))
    };
    match selection.provider {
        Provider::OpenAi => {
            openai::complete(client, &providers.openai, model()?, generation, prompt).await
        }
        Provider::Gemini => {
            gemini::complete(client, &providers.gemini, model()?, generation, prompt).await
        }
        Provider::Claude => {
            claude::complete(client, &providers.claude, model()?, generation, prompt).await
        }
        Provider::Grok => {
            grok::complete(client, &providers.grok, model()?, generation, prompt).await
        }
        Provider::MiniMax => {
            minimax::complete(client, &providers.minimax, model()?, generation, prompt).await
        }
        Provider::DeepSeek => {
            deepseek::complete(client, &providers.deepseek, model()?, generation, prompt).await
        }
        Provider::OpenAiCompatible => {
            openai_compatible::complete(
                client,
                &providers.openai_compatible,
                model()?,
                generation,
                prompt,
            )
            .await
        }
        Provider::OpenRouter => {
            openrouter::complete(client, &providers.openrouter, model()?, generation, prompt).await
        }
        Provider::LmStudio => {
            lm_studio::complete(client, &providers.lm_studio, model()?, generation, prompt).await
        }
        provider @ (Provider::DeepL | Provider::GoogleCloudTranslation | Provider::Caiyun) => {
            Err(Error::CompletionUnsupported {
                provider: provider.into(),
            })
        }
        Provider::Local => unreachable!("local completion has its own backend"),
    }
}

pub(crate) async fn translate(
    client: &Client,
    providers: &ProvidersConfig,
    selection: &ModelSelection,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    match selection.provider {
        Provider::DeepL => deepl::translate(client, &providers.deepl, request).await,
        Provider::GoogleCloudTranslation => {
            google_cloud::translate(client, &providers.google_cloud_translation, request).await
        }
        Provider::Caiyun => caiyun::translate(client, &providers.caiyun, request).await,
        provider => unreachable!("{provider} translates through structured completion"),
    }
}

pub(crate) async fn models(client: &Client, providers: &ProvidersConfig) -> Vec<Model> {
    let mut models = Vec::new();
    let pending: Vec<BoxFuture<'_, Result<Vec<Model>>>> = vec![
        openai::models(client).boxed(),
        gemini::models(client).boxed(),
        claude::models(client).boxed(),
        grok::models(client).boxed(),
        minimax::models(client).boxed(),
        deepseek::models(client).boxed(),
        openai_compatible::models(client, &providers.openai_compatible).boxed(),
        openrouter::models(client).boxed(),
        lm_studio::models(client, &providers.lm_studio).boxed(),
        deepl::models().boxed(),
        google_cloud::models().boxed(),
        caiyun::models().boxed(),
    ];
    for result in join_all(pending).await {
        append_models(&mut models, result);
    }
    models.sort_by(|left, right| {
        left.provider
            .to_string()
            .cmp(&right.provider.to_string())
            .then_with(|| left.model.cmp(&right.model))
    });
    models.dedup_by(|left, right| left.provider == right.provider && left.model == right.model);
    models
}

fn append_models(models: &mut Vec<Model>, result: Result<Vec<Model>>) {
    match result {
        Ok(mut provider_models) => models.append(&mut provider_models),
        Err(error) => tracing::warn!(%error, "failed to list translation models"),
    }
}

pub(super) async fn send_json<T: DeserializeOwned>(
    provider: &'static str,
    request: RequestBuilder,
) -> Result<T> {
    let response = request
        .send()
        .await
        .with_context(|| format!("{provider} request failed"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("failed to read {provider} response"))?;

    if !status.is_success() {
        let lower = text.to_ascii_lowercase();
        if status == StatusCode::TOO_MANY_REQUESTS
            || lower.contains("insufficient_quota")
            || lower.contains("resource_exhausted")
            || lower.contains("rate limit exceeded")
            || lower.contains("credit balance is too low")
        {
            return Err(Error::QuotaExceeded { provider });
        }
        return Err(Error::Api {
            provider,
            status: status.as_u16(),
            message: text.chars().take(4096).collect(),
        });
    }

    serde_json::from_str(&text)
        .with_context(|| format!("failed to decode {provider} response"))
        .map_err(Into::into)
}
