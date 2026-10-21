//! Android load profile for llama.cpp.
//!
//! Phones cannot afford the desktop defaults: the KV cache dominates resident
//! memory and only the performance cores help decode. This module owns the
//! RAM-class budgets from the mobile model profile table and derives the lean
//! batching, thread, and load settings from the device. A profile is only built
//! on Android; desktop callers pass `None`, so their behavior is byte-for-byte
//! unchanged. GPU offload is left to the selected backend: `model_params` forces
//! zero for a CPU backend or one without GPU offload and honors the configured
//! layer count for an accelerator.

use std::path::Path;

use anyhow::{Result, bail};

use super::{GenerationOptions, LlamaLoadMode, LoadOptions};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Scratch allowance added to the model file size when checking the budget.
const SCRATCH_ALLOWANCE: u64 = GIB;

// Nominal tiers from the profile table, lowered because Android reports less
// physical memory than the advertised capacity (an "8 GB" phone reports about
// 7.4 GiB of `MemTotal`).
const LOW_MAX: u64 = 7 * GIB;
const MID_MAX: u64 = 11 * GIB;

/// Device RAM class from the mobile model profile table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamClass {
    /// 6–8 GB devices.
    Low,
    /// 8–12 GB devices.
    Mid,
    /// 12 GB and above.
    High,
}

impl RamClass {
    /// Classifies total physical memory in bytes.
    #[must_use]
    pub fn from_total_bytes(bytes: u64) -> Self {
        if bytes < LOW_MAX {
            Self::Low
        } else if bytes < MID_MAX {
            Self::Mid
        } else {
            Self::High
        }
    }

    /// Peak translation budget for this class.
    #[must_use]
    pub fn budget_bytes(self) -> u64 {
        match self {
            Self::Low => 3584 * MIB,
            Self::Mid => 4608 * MIB,
            Self::High => 5632 * MIB,
        }
    }

    fn detect() -> Option<Self> {
        total_memory_bytes().map(Self::from_total_bytes)
    }

    fn profile(self) -> LoadProfile {
        // One vCPU count for both prompt and decode: llama.cpp does not benefit
        // from scheduling across the efficiency cluster on these models.
        let threads = performance_core_threads();
        let (n_batch, n_ubatch) = match self {
            Self::Low => (256, 128),
            Self::Mid => (512, 128),
            Self::High => (512, 256),
        };
        LoadProfile {
            load_mode: LlamaLoadMode::Mmap,
            n_threads: threads,
            n_threads_batch: threads,
            n_batch,
            n_ubatch,
            budget_bytes: self.budget_bytes(),
        }
    }
}

/// The resolved Android load profile applied by the llama.cpp loader.
#[derive(Debug, Clone, Copy)]
pub struct LoadProfile {
    pub load_mode: LlamaLoadMode,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub budget_bytes: u64,
}

impl LoadProfile {
    /// Resolves the profile for the current device, or `None` off Android.
    #[must_use]
    pub fn for_device() -> Option<Self> {
        cfg!(target_os = "android").then(|| {
            // Unknown memory still needs the lean mobile defaults; assume the
            // smallest class so an oversized model is refused rather than OOM.
            RamClass::detect().unwrap_or(RamClass::Low).profile()
        })
    }

    /// Applies the load-time settings to `options`.
    pub fn apply_load_options(&self, options: &mut LoadOptions) {
        options.load_mode = self.load_mode;
    }

    /// Applies the lean batching and thread settings.
    pub fn apply_generation_options(&self, options: &mut GenerationOptions) {
        options.n_batch = Some(self.n_batch);
        options.n_ubatch = Some(self.n_ubatch);
        options.n_threads = Some(self.n_threads);
        options.n_threads_batch = Some(self.n_threads_batch);
    }

    /// Refuses weights whose combined size plus scratch exceeds the device budget.
    pub fn check_budget(&self, model_path: &Path, projector_path: Option<&Path>) -> Result<()> {
        let size = file_size(model_path).saturating_add(projector_path.map_or(0, file_size));
        if size.saturating_add(SCRATCH_ALLOWANCE) > self.budget_bytes {
            let files = match projector_path {
                Some(projector) => {
                    format!("{} and {}", file_name(model_path), file_name(projector))
                }
                None => file_name(model_path),
            };
            bail!(
                "{files} total {:.1} GiB; this device's translation budget is {:.1} GiB — \
                 choose a smaller quantization",
                bytes_to_gib(size),
                bytes_to_gib(self.budget_bytes),
            );
        }
        Ok(())
    }
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / GIB as f64
}

/// Total physical memory from `/proc/meminfo`.
fn total_memory_bytes() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        let Some(rest) = line.strip_prefix("MemTotal:") else {
            continue;
        };
        let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        return Some(kb.saturating_mul(1024));
    }
    None
}

/// Counts the highest-frequency CPU cluster, falling back to half the online cores.
fn performance_core_threads() -> i32 {
    let frequencies = cpu_max_frequencies();
    let Some(max) = frequencies.iter().copied().max() else {
        // No frequency data: assume a typical big.LITTLE split where the
        // performance cluster is roughly half the online cores.
        return ((super::model::available_threads() + 1) / 2).max(1);
    };
    let count = frequencies.into_iter().filter(|value| *value == max).count();
    i32::try_from(count).unwrap_or(i32::MAX).max(1)
}

/// Per-CPU maximum frequencies, preferring the per-CPU file and falling back to
/// the policy-level file on kernels that only export the latter.
fn cpu_max_frequencies() -> Vec<u64> {
    let per_cpu = per_cpu_max_frequencies();
    if per_cpu.is_empty() {
        policy_max_frequencies()
    } else {
        per_cpu
    }
}

fn per_cpu_max_frequencies() -> Vec<u64> {
    let Ok(cpus) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return Vec::new();
    };
    let mut frequencies = Vec::new();
    for cpu in cpus.flatten() {
        let name = cpu.file_name();
        let name = name.to_string_lossy();
        let Some(index) = name.strip_prefix("cpu") else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(frequency) = std::fs::read_to_string(cpu.path().join("cpufreq/cpuinfo_max_freq"))
        else {
            continue;
        };
        if let Ok(frequency) = frequency.trim().parse::<u64>() {
            frequencies.push(frequency);
        }
    }
    frequencies
}

/// Expands each `cpufreq/policy*` directory into one entry per related CPU so the
/// caller can size the performance cluster by its frequency.
fn policy_max_frequencies() -> Vec<u64> {
    let Ok(policies) = std::fs::read_dir("/sys/devices/system/cpu/cpufreq") else {
        return Vec::new();
    };
    let mut frequencies = Vec::new();
    for policy in policies.flatten() {
        let name = policy.file_name();
        let name = name.to_string_lossy();
        let Some(index) = name.strip_prefix("policy") else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let path = policy.path();
        let Ok(frequency) = std::fs::read_to_string(path.join("scaling_max_freq")) else {
            continue;
        };
        let Ok(frequency) = frequency.trim().parse::<u64>() else {
            continue;
        };
        let count = std::fs::read_to_string(path.join("related_cpus"))
            .ok()
            .and_then(|cpus| cpu_list_len(cpus.trim()))
            .unwrap_or(1)
            .max(1);
        frequencies.extend(std::iter::repeat_n(frequency, count));
    }
    frequencies
}

/// Counts the CPUs in a sysfs list such as `0-3` or `0 1 2 3`.
fn cpu_list_len(list: &str) -> Option<usize> {
    let mut count = 0usize;
    for part in list.split_whitespace() {
        let (start, end) = match part.split_once('-') {
            Some((start, end)) => (start.parse::<u32>().ok()?, end.parse::<u32>().ok()?),
            None => {
                let cpu = part.parse::<u32>().ok()?;
                (cpu, cpu)
            }
        };
        count = count.saturating_add(end.checked_sub(start)? as usize + 1);
    }
    (count > 0).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_class_matches_mobile_profile_tiers() {
        assert_eq!(RamClass::from_total_bytes(6 * GIB), RamClass::Low);
        assert_eq!(RamClass::from_total_bytes(8 * GIB), RamClass::Mid);
        assert_eq!(RamClass::from_total_bytes(11 * GIB), RamClass::High);
        assert_eq!(RamClass::from_total_bytes(16 * GIB), RamClass::High);
    }

    #[test]
    fn higher_ram_class_has_a_larger_budget() {
        assert!(RamClass::Low.budget_bytes() < RamClass::Mid.budget_bytes());
        assert!(RamClass::Mid.budget_bytes() < RamClass::High.budget_bytes());
    }
}
