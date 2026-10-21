mod graph;
mod loader;
mod packages;

use std::{fmt::Display, hash::Hash, path::PathBuf};

use anyhow::Result;

use crate::{Device, Hardware};
use graph::{Component, Plan};
use packages::{Diffusion, Llama};

pub use packages::Torch;

mod sealed {
    pub trait Sealed {}
}

/// An immutable package managed by Koharu's runtime store.
#[allow(async_fn_in_trait)]
pub trait Package: sealed::Sealed + Copy + Display + Send + Sync + 'static {
    async fn install(self) -> Result<PathBuf>;
}

pub(crate) trait RuntimePackage: Package + std::fmt::Debug + Eq + Hash {
    const NAME: &'static str;

    fn dependencies(self, _hardware: &Hardware) -> Result<Vec<Component>> {
        Ok(Vec::new())
    }

    async fn activate(self) -> Result<()>;

    fn label(self) -> String {
        format!("{} {self}", Self::NAME)
    }
}

pub(crate) trait DiscoverablePackage: RuntimePackage {
    fn discover(hardware: &Hardware) -> Option<Self>;
}

/// A process-wide runtime capability requested by a consumer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Feature {
    Torch,
    Llama,
    Diffusion,
}

/// A discovered, dependency-ordered runtime plan.
#[derive(Debug)]
pub struct Runtime {
    plan: Plan,
    hardware: Hardware,
    features: Vec<Feature>,
}

impl Runtime {
    pub fn discover(features: impl IntoIterator<Item = Feature>) -> Result<Self> {
        let features = features.into_iter().collect::<Vec<_>>();
        let hardware = Hardware::discover();
        for candidate in hardware.candidates() {
            if let Some((plan, resolved)) = Self::plan(&features, &candidate)? {
                if resolved.len() < features.len() {
                    let missing = features
                        .iter()
                        .filter(|feature| !resolved.contains(feature))
                        .collect::<Vec<_>>();
                    tracing::warn!(?missing, "requested runtime features were not activated");
                }
                return Ok(Self {
                    plan,
                    hardware: candidate,
                    features: resolved,
                });
            }
        }
        anyhow::bail!("no device supports the requested runtime features")
    }

    /// The requested features that resolved on the selected device. Backends for
    /// requested features absent from this list were not activated and must not
    /// be initialized by the caller.
    #[must_use]
    pub fn features(&self) -> Vec<Feature> {
        self.features.clone()
    }

    fn plan(features: &[Feature], hardware: &Hardware) -> Result<Option<(Plan, Vec<Feature>)>> {
        let mut plan = Plan::default();
        let mut resolved = Vec::new();
        let mut previous = None;

        for feature in features {
            let Some(node) = (match feature {
                Feature::Torch => plan.require::<Torch>(hardware)?,
                Feature::Llama => plan.require::<Llama>(hardware)?,
                Feature::Diffusion => plan.require::<Diffusion>(hardware)?,
            }) else {
                continue;
            };
            resolved.push(*feature);
            if let Some(previous) = previous
                && previous != node
            {
                plan.sequence(previous, node);
            }
            previous = Some(node);
        }

        if resolved.is_empty() {
            return Ok(None);
        }
        Ok(Some((plan, resolved)))
    }

    /// Installs and activates packages, then returns the process-wide device.
    #[tracing::instrument(skip_all)]
    pub async fn initialize(self) -> Result<Device> {
        let device = self.hardware.device().cloned().unwrap_or_else(Device::cpu);
        self.plan.initialize(device).await
    }
}
