use std::sync::Arc;

use anyhow::{Context as _, Result};
use tauri::{AppHandle, Manager as _};
use tokio::sync::OnceCell;

use crate::commands::lifecycle::{ModelResources, ResourceChannel};

/// The process-wide ML runtime, initialized on the first pipeline request.
///
/// Loading models can fail on a device that has no runtime support at all. That failure must not
/// prevent the editor from opening, so the runtime is not initialized during setup.
pub(crate) struct Models {
    handle: AppHandle,
    pipeline: OnceCell<Arc<koharu_pipeline::Pipeline>>,
}

impl Models {
    pub(crate) fn new(handle: AppHandle) -> Self {
        Self {
            handle,
            pipeline: OnceCell::new(),
        }
    }

    pub(crate) async fn pipeline(&self) -> Result<Arc<koharu_pipeline::Pipeline>> {
        self.pipeline
            .get_or_try_init(|| async {
                koharu_ml::init()
                    .await
                    .context("failed to initialize the ML runtime")?;
                let device = koharu_ml::device(false);
                let pipeline = Arc::new(koharu_pipeline::Pipeline::load(device)?);
                self.subscribe_resources(&pipeline);
                Ok(pipeline)
            })
            .await
            .cloned()
    }

    fn subscribe_resources(&self, pipeline: &koharu_pipeline::Pipeline) {
        let mut resources = pipeline.subscribe_resources();
        let handle = self.handle.clone();
        drop(tauri::async_runtime::spawn(async move {
            while resources.changed().await.is_ok() {
                let snapshot = resources.borrow_and_update().clone();
                let resources = handle.state::<ResourceChannel>();
                let mut channel = resources.channel.lock();
                if let Some(current) = channel.as_ref()
                    && current.send(ModelResources::from(snapshot)).is_err()
                {
                    channel.take();
                }
            }
        }));
    }
}
