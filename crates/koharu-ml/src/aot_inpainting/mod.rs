//! BallonsTranslator-compatible AOT image inpainting.

mod model;
mod processor;

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
use koharu_torch::Device;

use crate::backend::TryIntoDevice;

use self::{model::Model, processor::Processor};

/// Longest side the AOT model runs at.
///
/// The model's 128-channel dilated convolutions materialize im2col columns at
/// crop resolution, which costs several gigabytes on mobile. Downscaling there
/// keeps the working set inside the phone's budget; desktop keeps the
/// higher-fidelity input.
const DEFAULT_MAX_SIDE: u32 = if cfg!(target_os = "android") {
    512
} else {
    2048
};

crate::model_repository!("mayocream/aot-inpainting" @ "cffe2346ac2b5ebe1f2d61335d602d12cc144c6f" {
    WEIGHTS = "model.safetensors"
});

#[derive(Debug)]
pub struct AotInpainting {
    model: Model,
    processor: Processor,
}

impl AotInpainting {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into_device()?;
        let weights_path = WEIGHTS
            .resolve()
            .await
            .context("failed to resolve AOT inpainting weights")?;
        let mut model = Model::new(device);
        model
            .load(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;
        Ok(Self {
            model,
            processor: Processor::new(device),
        })
    }

    pub fn inference(&self, image: &DynamicImage, mask: &GrayImage) -> Result<RgbImage> {
        self.inference_with_max_side(image, mask, DEFAULT_MAX_SIDE)
    }

    pub fn inference_with_max_side(
        &self,
        image: &DynamicImage,
        mask: &GrayImage,
        max_side: u32,
    ) -> Result<RgbImage> {
        #[cfg(target_os = "android")]
        let previous = koharu_torch::utils::user_enabled_mkldnn();
        #[cfg(target_os = "android")]
        koharu_torch::utils::set_user_enabled_mkldnn(true);
        let result =
            koharu_torch::no_grad(|| self.processor.call(&self.model, image, mask, max_side));
        #[cfg(target_os = "android")]
        koharu_torch::utils::set_user_enabled_mkldnn(previous);
        result
    }
}
