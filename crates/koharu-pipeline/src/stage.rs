use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Type,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Stage {
    Detection,
    Ocr,
    Translation,
    Dialogue,
    Inpainting,
}

impl Stage {
    /// Translation and dialogue share one LLM: they never run concurrently, and
    /// memory recovery for one must not unload the other.
    pub(crate) const fn lane(self) -> Self {
        match self {
            Self::Dialogue => Self::Translation,
            stage => stage,
        }
    }

    pub const ALL: [Self; 5] = [
        Self::Detection,
        Self::Ocr,
        Self::Translation,
        Self::Dialogue,
        Self::Inpainting,
    ];
}
