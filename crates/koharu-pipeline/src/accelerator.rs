use std::{sync::Arc, time::Duration};

use crate::{Stage, resources::ResourceMonitor, stages::Stages};

/// How many stages may hold weights at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryPolicy {
    /// Keep each stage's weights resident between pages (desktop).
    KeepLoaded,
    /// Evict every other stage before loading one, so only one model is
    /// resident at a time (mobile).
    EvictBetweenStages,
}

impl MemoryPolicy {
    pub(crate) fn for_platform() -> Self {
        if cfg!(target_os = "android") {
            Self::EvictBetweenStages
        } else {
            Self::KeepLoaded
        }
    }
}

pub(crate) struct AcceleratorGate {
    resources: Arc<ResourceMonitor>,
    lane: Option<Arc<tokio::sync::Semaphore>>,
    memory: MemoryPolicy,
}

pub(crate) struct AcceleratorPermit {
    _lane: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl AcceleratorGate {
    pub(crate) fn new(
        device: &koharu_ml::Device,
        resources: Arc<ResourceMonitor>,
        memory: MemoryPolicy,
    ) -> Self {
        let accelerated = device.backend != koharu_ml::Backend::Cpu
            && device.device_type != koharu_ml::DeviceType::Cpu;
        Self {
            resources,
            // Heterogeneous CUDA model pairs took 2.5-4x longer together than
            // back-to-back on the target workload, so accelerator work is
            // serialized while loaded weights remain resident between pages. The
            // mobile policy serializes for a different reason: only one stage may
            // be resident, which requires no other stage to be mid-inference.
            lane: (accelerated || memory == MemoryPolicy::EvictBetweenStages)
                .then(|| Arc::new(tokio::sync::Semaphore::new(1))),
            memory,
        }
    }

    /// Whether the caller must evict other stages before loading one.
    pub(crate) fn evicts_between_stages(&self) -> bool {
        self.memory == MemoryPolicy::EvictBetweenStages
    }

    pub(crate) async fn acquire(&self) -> AcceleratorPermit {
        let Some(lane) = self.lane.as_ref() else {
            return AcceleratorPermit::cpu();
        };
        AcceleratorPermit::accelerator(
            lane.clone()
                .acquire_owned()
                .await
                .expect("accelerator lane is never closed"),
        )
    }

    /// Releases the lane, unloads every other stage, and waits for a fresh
    /// resource sample before the caller retries once. Runs on the CPU lane too:
    /// an out-of-memory failure there is just as recoverable by eviction.
    pub(crate) async fn recover(&self, stage: Stage, stages: &Stages) -> AcceleratorPermit {
        let permit = self.acquire().await;
        if stages.unload_except(stage) {
            let mut changed = self.resources.subscribe();
            let _ = tokio::time::timeout(Duration::from_millis(600), changed.changed()).await;
        }
        permit
    }
}

impl AcceleratorPermit {
    fn accelerator(lane: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self { _lane: Some(lane) }
    }

    fn cpu() -> Self {
        Self { _lane: None }
    }
}
