use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tauri::{
    DynRuntime,
    plugin::{PluginHandle, mobile::PluginInvokeError},
};

use crate::ExportTarget;

/// Error code the Kotlin picker callbacks reject with when the user backs out.
const CANCELLED: &str = "cancelled";

#[derive(Clone)]
pub struct Android(PluginHandle<DynRuntime>);

impl Android {
    pub(crate) fn new(handle: PluginHandle<DynRuntime>) -> Self {
        Self(handle)
    }

    async fn invoke<T: DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> Result<T> {
        Ok(self.0.run_mobile_plugin_async(command, payload).await?)
    }

    /// Runs a command whose Kotlin callback reports user cancellation as a rejection with the
    /// `cancelled` code.
    async fn invoke_cancellable<T: DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> Result<Option<T>> {
        match self.0.run_mobile_plugin_async(command, payload).await {
            Ok(value) => Ok(Some(value)),
            Err(PluginInvokeError::InvokeRejected(error))
                if error.code.as_deref() == Some(CANCELLED) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Opens a multi-select document picker and returns the cached local copies.
    pub async fn pick_import(&self, extensions: &[&str]) -> Result<Option<Vec<PathBuf>>> {
        let response = self
            .invoke_cancellable::<ImportResponse>("pickImport", PickImport { extensions })
            .await?;
        Ok(response.and_then(|response| {
            (!response.paths.is_empty())
                .then(|| response.paths.into_iter().map(PathBuf::from).collect())
        }))
    }

    /// Opens a directory picker and returns the opaque tree URI.
    pub async fn pick_export_directory(&self, suggested_name: &str) -> Result<Option<String>> {
        Ok(self
            .invoke_cancellable::<UriResponse>("pickExportDirectory", PickExport { suggested_name })
            .await?
            .map(|response| response.uri))
    }

    /// Opens a create-document picker and returns the opaque document URI.
    pub async fn pick_export_file(
        &self,
        suggested_name: &str,
        mime: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .invoke_cancellable::<UriResponse>(
                "pickExportFile",
                PickExportFile {
                    suggested_name,
                    mime,
                },
            )
            .await?
            .map(|response| response.uri))
    }

    /// Copies local files into a destination returned by `pick_export_*`.
    pub async fn publish_export(&self, target: &ExportTarget, paths: Vec<PathBuf>) -> Result<()> {
        self.invoke(
            "publishExport",
            PublishExport {
                target,
                paths: &paths,
            },
        )
        .await
    }

    /// Opens the system share sheet for local files.
    pub async fn share(&self, paths: Vec<PathBuf>, mime: &str) -> Result<()> {
        self.invoke(
            "share",
            Share {
                paths: &paths,
                mime,
            },
        )
        .await
    }

    /// Blocking variants used by [`koharu_secrets`]'s synchronous backend trait.
    ///
    /// The Kotlin side only performs local Keystore work, so the wait never depends on the
    /// webview event loop.
    pub fn secret_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .run_mobile_plugin::<SecretValue>("secretGet", SecretKey { key })?
            .value)
    }

    pub fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<()>("secretSet", SecretSet { key, value })?;
        Ok(())
    }

    pub fn secret_delete(&self, key: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<()>("secretDelete", SecretKey { key })?;
        Ok(())
    }

    /// Moves the task to the back of the stack.
    pub async fn background(&self) -> Result<()> {
        self.invoke("background", ()).await
    }

    pub async fn foreground_start(&self, title: &str, text: &str) -> Result<()> {
        self.invoke("foregroundStart", Foreground { title, text })
            .await
    }

    pub async fn foreground_update(
        &self,
        title: &str,
        text: &str,
        progress: Option<(u32, u32)>,
    ) -> Result<()> {
        self.invoke(
            "foregroundUpdate",
            ForegroundProgress {
                title,
                text,
                progress,
            },
        )
        .await
    }

    pub async fn foreground_stop(&self) -> Result<()> {
        self.invoke("foregroundStop", ()).await
    }
}

#[derive(Serialize)]
struct PickImport<'a> {
    extensions: &'a [&'a str],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickExport<'a> {
    suggested_name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickExportFile<'a> {
    suggested_name: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
struct PublishExport<'a> {
    target: &'a ExportTarget,
    paths: &'a [PathBuf],
}

#[derive(Serialize)]
struct Share<'a> {
    paths: &'a [PathBuf],
    mime: &'a str,
}

#[derive(Serialize)]
struct SecretKey<'a> {
    key: &'a str,
}

#[derive(Serialize)]
struct SecretSet<'a> {
    key: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
struct Foreground<'a> {
    title: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct ForegroundProgress<'a> {
    title: &'a str,
    text: &'a str,
    progress: Option<(u32, u32)>,
}

#[derive(Deserialize)]
struct ImportResponse {
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct UriResponse {
    uri: String,
}

#[derive(Deserialize)]
struct SecretValue {
    #[serde(default)]
    value: Option<String>,
}
