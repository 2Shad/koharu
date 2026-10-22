use serde::{Deserialize, Serialize};

use crate::Device;

/// Runtime-owned settings for the Android device probe and the local load profile.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct RuntimeConfig {
    android_vulkan: bool,
    battery_saver: bool,
}

/// Vulkan is opt-in: mobile Adreno Vulkan/OpenCL is frequently slower than CPU
/// for the translation profile, so the CPU device is the default.
pub(super) fn vulkan_enabled() -> bool {
    read().is_some_and(|config| config.android_vulkan)
}

/// Whether the opt-in battery-saver load profile is enabled. `koharu-ml` applies
/// the reduced threads and budget; this only resolves the runtime setting.
pub(super) fn battery_saver_enabled() -> bool {
    read().is_some_and(|config| config.battery_saver)
}

fn read() -> Option<RuntimeConfig> {
    let config = koharu_config::load::<RuntimeConfig>("runtime").ok()?;
    config.read().ok().map(|config| *config)
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
