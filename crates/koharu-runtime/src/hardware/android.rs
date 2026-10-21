use serde::{Deserialize, Serialize};

use crate::Device;

/// Runtime-owned settings for the Android device probe.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct RuntimeConfig {
    android_vulkan: bool,
}

/// Vulkan is opt-in: mobile Adreno Vulkan/OpenCL is frequently slower than CPU
/// for the translation profile, so the CPU device is the default.
pub(super) fn vulkan_enabled() -> bool {
    koharu_config::load::<RuntimeConfig>("runtime")
        .and_then(|config| Ok(config.read()?.android_vulkan))
        .unwrap_or(false)
}

pub(super) fn cpu_device() -> Device {
    Device {
        memory_total: memory_total(),
        ..Device::cpu()
    }
}

/// `/proc/meminfo` `MemTotal` in bytes, or 0 when it cannot be read.
fn memory_total() -> usize {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(0, |kilobytes| kilobytes.saturating_mul(1024))
}
