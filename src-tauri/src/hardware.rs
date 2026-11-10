//! Hardware Profiling & Dynamic Model Recommendation Engine
//!
//! Automatically inspects host System RAM and dedicated GPU VRAM
//! on Linux systems to determine the optimal Ollama models and
//! queue concurrency policy on a per-install basis.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HardwareTier {
    Tier1Entry,
    Tier2Mid,
    Tier3Performance,
    Tier4Workstation,
}

impl HardwareTier {
    pub fn label(&self) -> &'static str {
        match self {
            HardwareTier::Tier1Entry => "Tier 1 (Entry / Integrated)",
            HardwareTier::Tier2Mid => "Tier 2 (Mid-Range Balanced)",
            HardwareTier::Tier3Performance => "Tier 3 (Performance)",
            HardwareTier::Tier4Workstation => "Tier 4 (Workstation)",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            HardwareTier::Tier1Entry => "Sub-6 GB VRAM or CPU-only. Strict serial execution with memory offload.",
            HardwareTier::Tier2Mid => "6 GB to 8 GB VRAM. Optimized for 7B fast coder in VRAM with hybrid 14B deep synthesis.",
            HardwareTier::Tier3Performance => "10 GB to 16 GB VRAM. Full 14B coder acceleration in VRAM with pipelined workers.",
            HardwareTier::Tier4Workstation => "20 GB+ VRAM. Full 32B deep reasoning in VRAM with high concurrency.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub is_dedicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecommendedModels {
    pub fast_model: String,
    pub coder_model: String,
    pub heavy_model: String,
    pub concurrency_mode: String,
    pub unload_after_run: bool,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareProfile {
    pub system_ram_total_mb: u64,
    pub system_ram_available_mb: u64,
    pub gpu: Option<GpuInfo>,
    pub hardware_tier: HardwareTier,
    pub tier_label: String,
    pub tier_description: String,
    pub recommended_models: RecommendedModels,
}

/// Reads System RAM from `/proc/meminfo` on Linux
pub fn read_system_ram() -> (u64, u64) {
    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        parse_meminfo(&content)
    } else {
        (8192, 4096)
    }
}

/// Parses MemTotal and MemAvailable lines from `/proc/meminfo`
pub fn parse_meminfo(content: &str) -> (u64, u64) {
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;

    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = extract_kb(line);
        } else if line.starts_with("MemAvailable:") {
            avail_kb = extract_kb(line);
        }
    }

    if avail_kb == 0 && total_kb > 0 {
        avail_kb = total_kb / 2;
    }

    (total_kb / 1024, avail_kb / 1024)
}

fn extract_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|val| val.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Probes NVIDIA GPU via `nvidia-smi`
pub fn probe_nvidia_gpu() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_output(&stdout)
}

/// Parses comma-separated output from `nvidia-smi`
pub fn parse_nvidia_smi_output(stdout: &str) -> Option<GpuInfo> {
    let line = stdout.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }

    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.len() < 3 {
        return None;
    }

    let name = parts[0].to_string();
    let total_mb = parts[1].parse::<u64>().ok()?;
    let free_mb = parts[2].parse::<u64>().ok()?;

    Some(GpuInfo {
        name,
        vram_total_mb: total_mb,
        vram_free_mb: free_mb,
        is_dedicated: total_mb >= 2048,
    })
}

/// Probes AMD GPU via sysfs (/sys/class/drm/card*/device/mem_info_vram_total)
pub fn probe_amd_gpu() -> Option<GpuInfo> {
    let drm_dir = Path::new("/sys/class/drm");
    if !drm_dir.exists() {
        return None;
    }

    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") && !name.contains('-') {
            let vram_path = entry.path().join("device/mem_info_vram_total");
            if vram_path.exists() {
                if let Ok(bytes_str) = fs::read_to_string(&vram_path) {
                    if let Ok(bytes) = bytes_str.trim().parse::<u64>() {
                        let total_mb = bytes / (1024 * 1024);
                        if total_mb >= 2048 {
                            let mut free_mb = total_mb;
                            let free_path = entry.path().join("device/mem_info_vram_used");
                            if let Ok(used_str) = fs::read_to_string(&free_path) {
                                if let Ok(used_bytes) = used_str.trim().parse::<u64>() {
                                    let used_mb = used_bytes / (1024 * 1024);
                                    free_mb = total_mb.saturating_sub(used_mb);
                                }
                            }
                            return Some(GpuInfo {
                                name: "AMD Radeon Dedicated GPU".to_string(),
                                vram_total_mb: total_mb,
                                vram_free_mb: free_mb,
                                is_dedicated: true,
                            });
                        }
                    }
                }
            }
        }
    }
    None
}

/// Evaluates hardware parameters into a structured profile with recommendations
pub fn evaluate_hardware(
    sys_ram_total_mb: u64,
    sys_ram_avail_mb: u64,
    gpu: Option<GpuInfo>,
) -> HardwareProfile {
    let (tier, recommended) = match &gpu {
        Some(g) if g.vram_total_mb >= 20_000 => (
            HardwareTier::Tier4Workstation,
            RecommendedModels {
                fast_model: "llama3.2:3b".to_string(),
                coder_model: "qwen2.5-coder:14b".to_string(),
                heavy_model: "qwen2.5-coder:32b".to_string(),
                concurrency_mode: "concurrent".to_string(),
                unload_after_run: false,
                rationale: format!(
                    "Workstation GPU detected ({:.1} GB VRAM). Fits 14B coder as fast worker and 32B for deep local synthesis with zero offload.",
                    g.vram_total_mb as f64 / 1024.0
                ),
            },
        ),
        Some(g) if g.vram_total_mb >= 10_000 => (
            HardwareTier::Tier3Performance,
            RecommendedModels {
                fast_model: "llama3.2:3b".to_string(),
                coder_model: "qwen2.5-coder:7b".to_string(),
                heavy_model: "qwen2.5-coder:14b".to_string(),
                concurrency_mode: "pipelined".to_string(),
                unload_after_run: false,
                rationale: format!(
                    "High-performance GPU detected ({:.1} GB VRAM). Fits 14B heavy model in VRAM without host RAM paging.",
                    g.vram_total_mb as f64 / 1024.0
                ),
            },
        ),
        Some(g) if g.vram_total_mb >= 5_500 => (
            HardwareTier::Tier2Mid,
            RecommendedModels {
                fast_model: "llama3.2:3b".to_string(),
                coder_model: "qwen2.5-coder:7b".to_string(),
                heavy_model: "qwen2.5-coder:14b".to_string(),
                concurrency_mode: "serial_worker".to_string(),
                unload_after_run: true,
                rationale: format!(
                    "Mid-range GPU detected ({} with {:.1} GB VRAM). qwen2.5-coder:7b fits completely in VRAM (~4.7 GB). 14B runs via hybrid system RAM offload.",
                    g.name,
                    g.vram_total_mb as f64 / 1024.0
                ),
            },
        ),
        Some(g) => (
            HardwareTier::Tier1Entry,
            RecommendedModels {
                fast_model: "llama3.2:1b".to_string(),
                coder_model: if sys_ram_total_mb >= 16_000 {
                    "qwen2.5-coder:3b".to_string()
                } else {
                    "qwen2.5-coder:1.5b".to_string()
                },
                heavy_model: "qwen2.5-coder:7b".to_string(),
                concurrency_mode: "strict_serial".to_string(),
                unload_after_run: true,
                rationale: format!(
                    "Limited VRAM detected ({:.1} GB). Using lightweight 1B/3B models to avoid driver OOM; 7B reserved for deep fallback.",
                    g.vram_total_mb as f64 / 1024.0
                ),
            },
        ),
        None => (
            HardwareTier::Tier1Entry,
            RecommendedModels {
                fast_model: "llama3.2:1b".to_string(),
                coder_model: if sys_ram_total_mb >= 24_000 {
                    "qwen2.5-coder:7b".to_string()
                } else if sys_ram_total_mb >= 12_000 {
                    "qwen2.5-coder:3b".to_string()
                } else {
                    "qwen2.5-coder:1.5b".to_string()
                },
                heavy_model: "qwen2.5-coder:7b".to_string(),
                concurrency_mode: "strict_serial".to_string(),
                unload_after_run: true,
                rationale: format!(
                    "No dedicated discrete GPU detected. CPU-only / unified memory execution active ({:.1} GB System RAM).",
                    sys_ram_total_mb as f64 / 1024.0
                ),
            },
        ),
    };

    HardwareProfile {
        system_ram_total_mb: sys_ram_total_mb,
        system_ram_available_mb: sys_ram_avail_mb,
        gpu,
        hardware_tier: tier.clone(),
        tier_label: tier.label().to_string(),
        tier_description: tier.description().to_string(),
        recommended_models: recommended,
    }
}

/// Probes the complete host hardware and evaluates the optimal profile
pub fn detect_hardware_profile() -> HardwareProfile {
    let (ram_total, ram_avail) = read_system_ram();
    let gpu = probe_nvidia_gpu().or_else(probe_amd_gpu);
    evaluate_hardware(ram_total, ram_avail, gpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_meminfo() {
        let meminfo = "MemTotal:       31935012 kB\nMemFree:         4033216 kB\nMemAvailable:   20764000 kB\nBuffers:              72 kB\n";
        let (total, avail) = parse_meminfo(meminfo);
        assert_eq!(total, 31186);
        assert_eq!(avail, 20277);
    }

    #[test]
    fn test_parse_nvidia_smi() {
        let smi = "NVIDIA GeForce RTX 4050 Laptop GPU, 6141, 5797\n";
        let gpu = parse_nvidia_smi_output(smi).expect("parsed gpu");
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 4050 Laptop GPU");
        assert_eq!(gpu.vram_total_mb, 6141);
        assert_eq!(gpu.vram_free_mb, 5797);
        assert!(gpu.is_dedicated);
    }

    #[test]
    fn test_frank_rtx4050_evaluates_to_tier2() {
        let gpu = GpuInfo {
            name: "NVIDIA GeForce RTX 4050 Laptop GPU".to_string(),
            vram_total_mb: 6141,
            vram_free_mb: 5797,
            is_dedicated: true,
        };
        let profile = evaluate_hardware(31186, 20277, Some(gpu));
        assert_eq!(profile.hardware_tier, HardwareTier::Tier2Mid);
        assert_eq!(profile.recommended_models.fast_model, "llama3.2:3b");
        assert_eq!(profile.recommended_models.coder_model, "qwen2.5-coder:7b");
        assert_eq!(profile.recommended_models.heavy_model, "qwen2.5-coder:14b");
        assert_eq!(profile.recommended_models.concurrency_mode, "serial_worker");
        assert!(profile.recommended_models.unload_after_run);
    }

    #[test]
    fn test_workstation_evaluates_to_tier4() {
        let gpu = GpuInfo {
            name: "NVIDIA RTX 4090".to_string(),
            vram_total_mb: 24576,
            vram_free_mb: 22000,
            is_dedicated: true,
        };
        let profile = evaluate_hardware(65536, 50000, Some(gpu));
        assert_eq!(profile.hardware_tier, HardwareTier::Tier4Workstation);
        assert_eq!(profile.recommended_models.coder_model, "qwen2.5-coder:14b");
        assert_eq!(profile.recommended_models.heavy_model, "qwen2.5-coder:32b");
        assert_eq!(profile.recommended_models.concurrency_mode, "concurrent");
        assert!(!profile.recommended_models.unload_after_run);
    }

    #[test]
    fn test_low_vram_evaluates_to_tier1() {
        let gpu = GpuInfo {
            name: "NVIDIA GTX 1050".to_string(),
            vram_total_mb: 4096,
            vram_free_mb: 3500,
            is_dedicated: true,
        };
        let profile = evaluate_hardware(16384, 8000, Some(gpu));
        assert_eq!(profile.hardware_tier, HardwareTier::Tier1Entry);
        assert_eq!(profile.recommended_models.fast_model, "llama3.2:1b");
        assert_eq!(profile.recommended_models.coder_model, "qwen2.5-coder:3b");
        assert_eq!(profile.recommended_models.heavy_model, "qwen2.5-coder:7b");
    }

    #[test]
    fn test_cpu_only_evaluates_to_tier1_with_safe_models() {
        let profile = evaluate_hardware(16384, 8000, None);
        assert_eq!(profile.hardware_tier, HardwareTier::Tier1Entry);
        assert_eq!(profile.recommended_models.fast_model, "llama3.2:1b");
        assert_eq!(profile.recommended_models.coder_model, "qwen2.5-coder:3b");
    }
}
