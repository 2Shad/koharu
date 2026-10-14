use std::{io::Cursor, sync::Arc};

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageEncoder, RgbImage, codecs::jpeg::JpegEncoder};

const MAX_IMAGE_DIMENSION: u32 = 2048;
const JPEG_QUALITY: u8 = 88;

#[derive(Debug, Clone)]
pub struct Prompt {
    /// Provider schema name; `[a-z0-9_]+`.
    pub name: &'static str,
    pub system: String,
    pub user: String,
    /// Strict-mode schema: every object sets `additionalProperties: false` and
    /// requires all properties.
    pub schema: serde_json::Value,
    pub image: Option<Arc<DynamicImage>>,
}

impl Prompt {
    /// Drops the image without `vision`; otherwise caps its longest side at
    /// `MAX_IMAGE_DIMENSION`.
    pub(crate) fn prepare_image(&mut self, vision: bool) -> anyhow::Result<()> {
        let Some(image) = self.image.take().filter(|_| vision) else {
            return Ok(());
        };
        let (width, height) = (image.width(), image.height());
        let longest = width.max(height);
        if longest <= MAX_IMAGE_DIMENSION {
            self.image = Some(image);
            return Ok(());
        }
        let resized_width =
            (u64::from(width) * u64::from(MAX_IMAGE_DIMENSION) / u64::from(longest)).max(1) as u32;
        let resized_height =
            (u64::from(height) * u64::from(MAX_IMAGE_DIMENSION) / u64::from(longest)).max(1) as u32;
        let source = image.to_rgb8();
        let mut resized = RgbImage::new(resized_width, resized_height);
        Resizer::new()
            .resize(
                &source,
                &mut resized,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(false),
            )
            .context("failed to resize prompt image")?;
        self.image = Some(Arc::new(DynamicImage::ImageRgb8(resized)));
        Ok(())
    }
}

pub(crate) struct EncodedImage {
    pub(crate) data: String,
}

impl EncodedImage {
    pub(crate) fn data_url(&self) -> String {
        format!("data:image/jpeg;base64,{}", self.data)
    }
}

pub(crate) fn encode_image(image: &DynamicImage) -> anyhow::Result<EncodedImage> {
    let rgb = image.to_rgb8();
    let mut bytes = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .context("failed to encode prompt image")?;
    Ok(EncodedImage {
        data: STANDARD.encode(bytes.into_inner()),
    })
}

#[cfg(test)]
pub(crate) fn test_prompt(image: Option<DynamicImage>) -> Prompt {
    Prompt {
        name: "test_prompt",
        system: "system".to_owned(),
        user: "user".to_owned(),
        schema: serde_json::json!({
            "type": "object",
            "properties": { "answer": { "type": "string" } },
            "required": ["answer"],
            "additionalProperties": false
        }),
        image: image.map(Arc::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_image_drops_images_without_vision() {
        let mut prompt = test_prompt(Some(DynamicImage::new_rgb8(4, 4)));
        prompt.prepare_image(false).unwrap();
        assert!(prompt.image.is_none());
    }

    #[test]
    fn prepare_image_bounds_the_longest_side() {
        let mut prompt = test_prompt(Some(DynamicImage::new_rgb8(4096, 1024)));
        prompt.prepare_image(true).unwrap();
        let image = prompt.image.unwrap();
        assert_eq!((image.width(), image.height()), (2048, 512));
    }
}
