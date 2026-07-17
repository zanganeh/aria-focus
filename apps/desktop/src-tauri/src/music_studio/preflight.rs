//! Music Studio preflight: detect local hardware and compare it against the
//! minimum requirements the packaged runtime needs.
//!
//! Detection uses safe, native system queries only. No shell is invoked and no
//! arbitrary packages are installed. `nvidia-smi` is the single well-known
//! NVIDIA system tool that may be called (with explicit arguments, never
//! through a shell) to read total VRAM. When a probe cannot determine a value
//! it returns `None` ("unknown") and the preflight never blocks on unknown
//! hardware, so non-Windows and CI builds stay green and GPU-independent.

use std::path::Path;

use music_studio_domain::StudioHardwareInfo;

/// One GiB safety margin on top of the signed manifest's required bytes.
pub const DISK_SAFETY_MARGIN_BYTES: u64 = 1024 * 1024 * 1024;

/// Minimum system RAM for the packaged Music Studio runtime.
pub const MIN_MEMORY_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Minimum CUDA VRAM for the packaged Music Studio runtime.
pub const MIN_VRAM_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The supported packaged architecture.
pub const SUPPORTED_ARCHITECTURE: &str = "x86_64";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardwareProbe {
    pub architecture: Option<String>,
    pub memory_bytes: Option<u64>,
    pub accelerator: Option<String>,
    pub vram_bytes: Option<u64>,
    /// `None` = could not determine, `Some(true)` = CUDA-capable NVIDIA GPU
    /// detected, `Some(false)` = definitively no CUDA GPU detected.
    pub cuda: Option<bool>,
}

/// Detect local hardware. Conservative: any value that cannot be safely
/// probed is left as `None` rather than guessed.
pub fn probe_hardware() -> HardwareProbe {
    HardwareProbe {
        architecture: Some(std::env::consts::ARCH.to_owned()),
        memory_bytes: detect_memory_bytes(),
        accelerator: None,
        vram_bytes: None,
        cuda: None,
    }
    .with_cuda()
}

impl HardwareProbe {
    fn with_cuda(mut self) -> HardwareProbe {
        match detect_cuda_vram_mib() {
            Some(mib) => {
                self.cuda = Some(true);
                self.accelerator = Some("NVIDIA CUDA".to_owned());
                self.vram_bytes = Some(mib * 1024 * 1024);
            }
            None => {
                // nvidia-smi not available: leave CUDA unknown so we do not
                // block machines we cannot confidently inspect.
            }
        }
        self
    }
}

/// Free disk space at `path` (the directory the runtime will live in).
pub fn free_disk_bytes(path: &Path) -> Option<u64> {
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    fs2::free_space(&target).ok()
}

/// Hardware-only reasons (architecture, RAM, CUDA, VRAM). These apply both
/// before setup and to an already-installed runtime, because generation cannot
/// succeed on hardware that fails them. Unknown probes never produce a reason.
pub fn preflight_hardware_reasons(probe: &HardwareProbe) -> Vec<String> {
    let mut reasons = Vec::new();

    if let Some(arch) = &probe.architecture {
        if arch != SUPPORTED_ARCHITECTURE {
            reasons.push(format!(
                "Architecture {arch} is not supported; Music Studio needs a {SUPPORTED_ARCHITECTURE} (x64) device."
            ));
        }
    }
    if let Some(memory) = probe.memory_bytes {
        if memory < MIN_MEMORY_BYTES {
            let have_gib = memory / (1024 * 1024 * 1024);
            let need_gib = MIN_MEMORY_BYTES / (1024 * 1024 * 1024);
            reasons.push(format!(
                "System RAM is about {have_gib} GiB; Music Studio needs at least {need_gib} GiB."
            ));
        }
    }
    match probe.cuda {
        Some(false) => {
            reasons.push(
                "No NVIDIA CUDA GPU was detected; Music Studio needs an NVIDIA CUDA GPU with at least 8 GiB VRAM."
                    .to_owned(),
            );
        }
        Some(true) => {
            if let Some(vram) = probe.vram_bytes {
                if vram < MIN_VRAM_BYTES {
                    let have_gib = vram / (1024 * 1024 * 1024);
                    let need_gib = MIN_VRAM_BYTES / (1024 * 1024 * 1024);
                    reasons.push(format!(
                        "GPU VRAM is about {have_gib} GiB; Music Studio needs at least {need_gib} GiB of VRAM."
                    ));
                }
            }
        }
        None => {}
    }
    reasons
}

/// Full preflight reasons: hardware plus free-disk space for setup. The disk
/// gate only applies before install (an already-installed runtime does not need
/// more free disk). Unknown probes never produce a reason.
pub fn preflight_reasons(
    probe: &HardwareProbe,
    min_free_disk_bytes: u64,
    free_bytes: Option<u64>,
) -> Vec<String> {
    let mut reasons = preflight_hardware_reasons(probe);
    if let Some(free) = free_bytes {
        if free < min_free_disk_bytes {
            let have_gb = free / (1000 * 1000 * 1000);
            let need_gb = min_free_disk_bytes / (1000 * 1000 * 1000);
            reasons.push(format!(
                "Free disk is about {have_gb} GB; Music Studio needs about {need_gb} GB free for setup."
            ));
        }
    }
    reasons
}

pub fn hardware_dto(probe: &HardwareProbe) -> StudioHardwareInfo {
    StudioHardwareInfo {
        architecture: probe.architecture.clone(),
        memory_bytes: probe.memory_bytes,
        accelerator: probe.accelerator.clone(),
        vram_bytes: probe.vram_bytes,
        cuda: probe.cuda,
    }
}

#[cfg(windows)]
fn detect_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut mem: MEMORYSTATUSEX = std::mem::zeroed();
        mem.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut mem) != 0 {
            Some(mem.ullTotalPhys)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
fn detect_memory_bytes() -> Option<u64> {
    None
}

/// Read total VRAM in MiB from `nvidia-smi`, or `None` if it is unavailable.
/// Uses explicit arguments and never invokes a shell.
fn detect_cuda_vram_mib() -> Option<u64> {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let line = stdout.lines().next()?;
    let trimmed = line.trim();
    // Some drivers append a unit suffix even with `nounits`; strip non-digits.
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_hardware_never_blocks_preflight() {
        let probe = HardwareProbe::default();
        let reasons = preflight_reasons(&probe, MIN_VRAM_BYTES, None);
        assert!(reasons.is_empty(), "{reasons:?}");
    }

    #[test]
    fn insufficient_ram_is_actionable_when_detected() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(8 * 1024 * 1024 * 1024),
            cuda: Some(true),
            vram_bytes: Some(12 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let reasons = preflight_reasons(&probe, 0, Some(100 * 1024 * 1024 * 1024));
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("RAM"));
    }

    #[test]
    fn insufficient_vram_is_actionable_when_cuda_detected() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: Some(true),
            vram_bytes: Some(4 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let reasons = preflight_reasons(&probe, 0, Some(100 * 1024 * 1024 * 1024));
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("VRAM"));
    }

    #[test]
    fn missing_cuda_is_actionable_only_when_definitively_absent() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: Some(false),
            ..Default::default()
        };
        let reasons = preflight_reasons(&probe, 0, Some(100 * 1024 * 1024 * 1024));
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("CUDA"));
    }

    #[test]
    fn unknown_cuda_does_not_block() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: None,
            ..Default::default()
        };
        let reasons = preflight_reasons(&probe, 0, Some(100 * 1024 * 1024 * 1024));
        assert!(reasons.is_empty(), "{reasons:?}");
    }

    #[test]
    fn unsupported_architecture_is_actionable_when_detected() {
        let probe = HardwareProbe {
            architecture: Some("aarch64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: Some(true),
            vram_bytes: Some(12 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let reasons = preflight_reasons(&probe, 0, Some(100 * 1024 * 1024 * 1024));
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("Architecture"));
    }

    #[test]
    fn insufficient_disk_is_actionable_when_detected() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: Some(true),
            vram_bytes: Some(12 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let min_disk = 15 * 1024 * 1024 * 1024;
        let reasons = preflight_reasons(&probe, min_disk, Some(1024 * 1024 * 1024));
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("disk"));
    }

    #[test]
    fn fully_capable_hardware_passes() {
        let probe = HardwareProbe {
            architecture: Some("x86_64".into()),
            memory_bytes: Some(32 * 1024 * 1024 * 1024),
            cuda: Some(true),
            vram_bytes: Some(12 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let reasons = preflight_reasons(
            &probe,
            15 * 1024 * 1024 * 1024,
            Some(100 * 1024 * 1024 * 1024),
        );
        assert!(reasons.is_empty(), "{reasons:?}");
    }

    #[test]
    fn vram_parser_strips_unit_suffix() {
        // Simulates a driver that ignores `nounits` and prints "12288 MiB".
        let parsed: Option<u64> = "12288 MiB"
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .ok();
        assert_eq!(parsed, Some(12288));
    }
}
