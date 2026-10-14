use anyhow::Context as _;
use koharu_desktop::Desktop;
use koharu_scene::EntityId;
use koharu_speech::{Line, Speech, SpeechModel};
use tauri::{State, ipc::IpcResponse};

use super::{
    Accelerator, ChannelExt as _, Error,
    canvas::CanvasChannel,
    editing::synchronize_canvas,
    project::{CurrentProject, Project},
};

/// WAV bytes, delivered to the webview as an `ArrayBuffer`.
#[derive(specta::Type)]
pub(crate) struct SpeechBytes(#[specta(type = Vec<u8>)] Vec<u8>);

impl IpcResponse for SpeechBytes {
    fn body(self) -> tauri::Result<tauri::ipc::InvokeResponseBody> {
        Ok(self.0.into())
    }
}

/// Reuses the stored recording unless `regenerate` is set or the line changed.
#[tracing::instrument(target = "koharu_metrics", name = "speech_played", skip_all)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn speak(
    layer: EntityId,
    regenerate: bool,
    speech: State<'_, Speech>,
    accelerator: State<'_, Accelerator>,
    pipeline: State<'_, koharu_pipeline::Pipeline>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<SpeechBytes, Error> {
    let (snapshot, line) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        let snapshot = project.snapshot();
        let line = Project::spoken_line(&snapshot, layer)?;
        (snapshot, line)
    };
    if !regenerate && let Some(asset) = Project::speech(&snapshot, &line)? {
        return Ok(SpeechBytes(snapshot.read_blob(asset.blob).await?.to_vec()));
    }

    // Drama 3 needs most of the accelerator. The project lock is released
    // during synthesis; `store_speech` drops stale results.
    let audio = {
        let _lease = accelerator.lock().await;
        pipeline.unload_models().await;
        speech
            .synthesize(Line {
                script: &line.script,
                character: line.character.as_deref(),
            })
            .await?
    };
    // A recording is a cache entry, not an undo step.
    let stored = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project
            .store_speech(layer, &line, audio.wav.clone())
            .await?;
        commit.map(|commit| (commit, project.active_page()))
    };
    if let Some((commit, page)) = stored {
        canvas_channel
            .channel
            .publish(synchronize_canvas(&desktop, &commit, page).await?);
    }
    Ok(SpeechBytes(audio.wav))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_speech_model(speech: State<'_, Speech>) -> Result<SpeechModel, Error> {
    Ok(speech.model()?)
}

#[tracing::instrument(target = "koharu_metrics", name = "speech_installed", skip_all)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn install_speech(speech: State<'_, Speech>) -> Result<SpeechModel, Error> {
    speech.install().await?;
    Ok(speech.model()?)
}
