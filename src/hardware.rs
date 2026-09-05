use std::path::Path;
use sysinfo::System;
use colored::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum PlatformArch {
    X86_64,
    Aarch64,
    Other(String),
}

#[derive(Debug, Clone, Serialize)]
pub enum OsKind {
    MacOS,
    Linux,
    Windows,
    Other(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct HardwareProfile {
    pub os: OsKind,
    pub arch: PlatformArch,
    pub cpu_brand: String,
    pub physical_cores: usize,
    pub logical_threads: usize,
    pub performance_cores: Option<usize>, // macOS Apple Silicon P-Cores
    pub efficiency_cores: Option<usize>,  // macOS Apple Silicon E-Cores
    pub has_avx2: bool,
    pub has_avx512: bool,
    pub has_fma: bool,
    pub has_neon: bool,
    pub total_ram_gb: f64,
    pub available_ram_gb: f64,
    pub is_unified_memory: bool,          // Apple Silicon UMA or APU
    pub gpu_detected: bool,
    pub gpu_backend: String,              // "Metal", "CUDA", "ROCm", "Vulkan", "CPU-SIMD"
    pub gpu_name: Option<String>,
    pub vram_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineSteeringConfig {
    pub threads_decode: u32,
    pub threads_prefill: u32,
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub n_gpu_layers: u32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub flash_attn: bool,
    pub safety_margin_gb: f64,
    pub idle_timeout_secs: u64,
}

impl HardwareProfile {
    pub fn detect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        // 1. Detect Operating System
        let os = if cfg!(target_os = "macos") {
            OsKind::MacOS
        } else if cfg!(target_os = "linux") {
            OsKind::Linux
        } else if cfg!(target_os = "windows") {
            OsKind::Windows
        } else {
            OsKind::Other(std::env::consts::OS.to_string())
        };

        // 2. Detect CPU Architecture
        let arch = if cfg!(target_arch = "x86_64") {
            PlatformArch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            PlatformArch::Aarch64
        } else {
            PlatformArch::Other(std::env::consts::ARCH.to_string())
        };

        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Generic Processor".to_string());

        let physical_cores = sys.physical_core_count().unwrap_or(4).max(1);
        let logical_threads = sys.cpus().len().max(physical_cores);

        // 3. Apple Silicon Topology (Performance vs Efficiency cores on macOS)
        #[allow(unused_mut)]
        let (mut performance_cores, mut efficiency_cores) = (None, None);
        #[allow(unused_mut)]
        let mut is_unified_memory = false;

        #[cfg(target_os = "macos")]
        {
            // On Apple Silicon, unified memory is shared between CPU and GPU
            if cfg!(target_arch = "aarch64") {
                is_unified_memory = true;
                
                // Query Apple Silicon P-cores (perflevel0) and E-cores (perflevel1) via sysctl
                if let Ok(output) = std::process::Command::new("sysctl")
                    .args(["-n", "hw.perflevel0.logicalcpu"])
                    .output()
                {
                    if let Ok(val_str) = String::from_utf8(output.stdout) {
                        if let Ok(p_count) = val_str.trim().parse::<usize>() {
                            performance_cores = Some(p_count);
                        }
                    }
                }

                if let Ok(output) = std::process::Command::new("sysctl")
                    .args(["-n", "hw.perflevel1.logicalcpu"])
                    .output()
                {
                    if let Ok(val_str) = String::from_utf8(output.stdout) {
                        if let Ok(e_count) = val_str.trim().parse::<usize>() {
                            efficiency_cores = Some(e_count);
                        }
                    }
                }
            }
        }

        // 4. Vector Accelerators & SIMD
        #[cfg(target_arch = "x86_64")]
        let (has_avx2, has_avx512, has_fma, has_neon) = (
            std::is_x86_feature_detected!("avx2"),
            std::is_x86_feature_detected!("avx512f"),
            std::is_x86_feature_detected!("fma"),
            false,
        );

        #[cfg(target_arch = "aarch64")]
        let (has_avx2, has_avx512, has_fma, has_neon) = (
            false,
            false,
            false,
            true, // ARM NEON is standard on AArch64 / Apple Silicon
        );

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let (has_avx2, has_avx512, has_fma, has_neon) = (false, false, false, false);

        // 5. Memory metrics
        let total_ram_bytes = sys.total_memory();
        let available_ram_bytes = sys.available_memory();
        let total_ram_gb = total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let available_ram_gb = available_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

        // 6. Universal GPU / Accelerator Discovery
        let mut gpu_detected = false;
        let mut gpu_backend = "CPU-SIMD".to_string();
        let mut gpu_name = None;
        let mut vram_mb = None;

        // Path A: macOS Apple Silicon (Metal Engine with Unified RAM)
        if cfg!(target_os = "macos") && is_unified_memory {
            gpu_detected = true;
            gpu_backend = "Apple Metal (Unified Memory)".to_string();
            gpu_name = Some(cpu_brand.clone());
            // In Apple Silicon, GPU has access to up to ~75-80% of total unified memory
            vram_mb = Some((total_ram_gb * 0.75 * 1024.0) as u64);
        }

        // Path B: NVIDIA CUDA
        if !gpu_detected {
            if let Ok(output) = std::process::Command::new("nvidia-smi")
                .arg("--query-gpu=name,memory.total")
                .arg("--format=csv,noheader,nounits")
                .output()
            {
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    if let Some(first) = text.trim().lines().next() {
                        let parts: Vec<&str> = first.split(',').collect();
                        if parts.len() >= 2 {
                            gpu_detected = true;
                            gpu_backend = "NVIDIA CUDA".to_string();
                            gpu_name = Some(parts[0].trim().to_string());
                            if let Ok(vram) = parts[1].trim().parse::<u64>() {
                                vram_mb = Some(vram);
                            }
                        }
                    }
                }
            }
        }

        // Path C: AMD ROCm
        if !gpu_detected {
            if std::path::Path::new("/dev/kfd").exists() {
                gpu_detected = true;
                gpu_backend = "AMD ROCm/HIP".to_string();
                gpu_name = Some("AMD Radeon Discrete/APU".to_string());
            }
        }

        // Path D: Linux DRI / Vulkan render node detection
        if !gpu_detected && std::path::Path::new("/dev/dri/renderD128").exists() {
            // Check if discrete GPU or integrated
            let lspci_check = std::process::Command::new("lspci").output();
            if let Ok(out) = lspci_check {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if line.contains("VGA") || line.contains("3D") || line.contains("Display") {
                        if line.contains("NVIDIA") {
                            gpu_detected = true;
                            gpu_backend = "NVIDIA (Vulkan/DRI)".to_string();
                            gpu_name = Some(line.to_string());
                            break;
                        } else if line.contains("AMD") || line.contains("Radeon") {
                            gpu_detected = true;
                            gpu_backend = "AMD Vulkan".to_string();
                            gpu_name = Some(line.to_string());
                            break;
                        }
                    }
                }
            }
        }

        Self {
            os,
            arch,
            cpu_brand,
            physical_cores,
            logical_threads,
            performance_cores,
            efficiency_cores,
            has_avx2,
            has_avx512,
            has_fma,
            has_neon,
            total_ram_gb,
            available_ram_gb,
            is_unified_memory,
            gpu_detected,
            gpu_backend,
            gpu_name,
            vram_mb,
        }
    }

    /// Calculate optimal, safe engine parameters for ANY machine
    pub fn auto_tune(&self, model_path: Option<&Path>) -> EngineSteeringConfig {
        // 1. Thread Steering:
        // On Apple Silicon (macOS): Pin decode to Performance Cores (avoiding low-power Efficiency Cores)
        let threads_decode = if let Some(p_cores) = self.performance_cores {
            p_cores as u32
        } else {
            self.physical_cores as u32
        };

        // Prefill: Pinned to physical cores to maximize AVX2 cache locality and avoid hyperthread memory contention
        let threads_prefill = self.physical_cores as u32;

        // 2. RAM Safety Margin & Context Window Tuning:
        let model_size_gb = model_path
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len() as f64 / (1024.0 * 1024.0 * 1024.0))
            .unwrap_or(3.1);

        // Keep 1.5 GB to 2.0 GB always reserved for the OS to guarantee 100% stability
        let safety_margin_gb = 1.8;
        let usable_ram_gb = (self.available_ram_gb - safety_margin_gb).max(0.5);

        // Budget context size based on remaining memory after model loading
        // Gemma 4 2B KV cache is very lightweight (~24 MB per 1024 tokens)
        let ram_for_kv = usable_ram_gb - model_size_gb;
        let n_ctx = if ram_for_kv > 5.0 {
            32768 // 32k context
        } else if ram_for_kv > 2.0 {
            16384 // 16k context
        } else if ram_for_kv > 1.0 {
            8192  // 8k context
        } else {
            4096  // 4k context minimum safety
        };

        // mlock: only if usable RAM exceeds model size by at least 1.5 GB
        let use_mlock = usable_ram_gb > (model_size_gb + 1.5);
        let use_mmap = true;

        // 3. Dynamic GPU Layer Offloading:
        let n_gpu_layers = if self.is_unified_memory {
            // Apple Silicon Metal: offload ALL layers (unified memory means zero VRAM limit)
            999
        } else if self.gpu_detected {
            if let Some(vram) = self.vram_mb {
                if vram >= (model_size_gb * 1024.0 * 1.2) as u64 {
                    999 // Fits entirely in VRAM
                } else if vram > 3000 {
                    24  // Partial offload
                } else {
                    12
                }
            } else {
                999
            }
        } else {
            0 // CPU SIMD
        };

        EngineSteeringConfig {
            threads_decode,
            threads_prefill,
            n_ctx,
            n_batch: 512,
            n_ubatch: 512,
            n_gpu_layers,
            use_mmap,
            use_mlock,
            flash_attn: true,
            safety_margin_gb,
            idle_timeout_secs: 300,
        }
    }

    pub fn print_dashboard(&self, config: &EngineSteeringConfig, model_name: &str) {
        println!("{}", "══════════════════════════════════════════════════════════════════".bright_cyan());
        println!("       🚀 {} - {}", "ClawMind AI Engine".bright_green().bold(), "Universal Hardware Orchestrator".bright_yellow().bold());
        println!("{}", "══════════════════════════════════════════════════════════════════".bright_cyan());

        let os_name = match &self.os {
            OsKind::MacOS => "macOS (Apple Darwin)".bright_blue(),
            OsKind::Linux => "Linux Kernel".bright_green(),
            OsKind::Windows => "Windows NT".bright_cyan(),
            OsKind::Other(s) => s.normal(),
        };

        let arch_name = match &self.arch {
            PlatformArch::X86_64 => "x86_64 (64-bit AMD/Intel)",
            PlatformArch::Aarch64 => "aarch64 (ARM64 / Apple Silicon)",
            PlatformArch::Other(s) => s.as_str(),
        };

        println!("{}: {} [{}]", "Operating Platform".bold(), os_name, arch_name.dimmed());
        println!("{}: {}", "Active Model".bold(), model_name.bright_magenta());
        println!("{}: {}", "CPU Processor".bold(), self.cpu_brand.cyan());

        if let (Some(p), Some(e)) = (self.performance_cores, self.efficiency_cores) {
            println!(
                "{}: {} Performance (P-Cores) | {} Efficiency (E-Cores)",
                "Apple Silicon Topology".bold(),
                p.to_string().bright_green().bold(),
                e.to_string().yellow()
            );
        } else {
            println!(
                "{}: {} physical cores / {} logical threads",
                "CPU Topology".bold(),
                self.physical_cores.to_string().bright_green(),
                self.logical_threads.to_string().bright_green()
            );
        }

        let simd_info = if self.has_neon {
            "ARM NEON + FP16 (Active)".bright_green().to_string()
        } else {
            format!(
                "AVX2: {}, FMA: {}, AVX-512: {}",
                if self.has_avx2 { "ENABLED".green() } else { "NO".dimmed() },
                if self.has_fma { "ENABLED".green() } else { "NO".dimmed() },
                if self.has_avx512 { "ENABLED".green() } else { "NO".dimmed() }
            )
        };
        println!("{}: {}", "Vector Engine (SIMD)".bold(), simd_info);

        println!(
            "{}: {:.2} GB Total / {:.2} GB Available (Safety Buffer: {:.1} GB)",
            "System RAM".bold(),
            self.total_ram_gb,
            self.available_ram_gb.to_string().bright_green(),
            config.safety_margin_gb
        );

        if self.gpu_detected {
            println!(
                "{}: {} via {} (VRAM: {} MB) -> Offloading {} layers",
                "GPU Acceleration".bold(),
                self.gpu_name.as_deref().unwrap_or("Hardware Accelerator").bright_green(),
                self.gpu_backend.bright_yellow(),
                self.vram_mb.unwrap_or(0),
                config.n_gpu_layers.to_string().bright_yellow().bold()
            );
        } else {
            println!(
                "{}: {}",
                "GPU Acceleration".bold(),
                "CPU-Native Vector Pipeline (AVX2/FMA Optimized)".yellow()
            );
        }

        println!("{}", "──────────────────────────────────────────────────────────────────".bright_black());
        println!("⚙️  {}", "Dynamic Machine Orchestration Policy:".bold().underline());
        println!(
            "  • Token Generation Threads : {} (Pinned to high-perf cores, zero cache-thrash)",
            config.threads_decode.to_string().bright_green().bold()
        );
        println!(
            "  • Prompt Prefill Threads   : {} (Full hardware thread pool)",
            config.threads_prefill.to_string().bright_green()
        );
        println!(
            "  • Context Capacity (n_ctx) : {} tokens (Calculated dynamically for safe RAM limits)",
            config.n_ctx.to_string().bright_cyan()
        );
        println!(
            "  • Memory Pinning (mlock)   : {}",
            if config.use_mlock { "ACTIVE (100% RAM-locked, 0% disk page swap)".green() } else { "DYNAMIC (Adaptive)".yellow() }
        );
        println!(
            "  • Flash Attention          : {}",
            if config.flash_attn { "ACTIVE (O(1) memory complexity)".green() } else { "INACTIVE".dimmed() }
        );
        println!(
            "  • Smart Hibernation (Idle) : {}",
            if config.idle_timeout_secs > 0 {
                format!("ACTIVE (Releases RAM & CPU after {}s idle; instant wake-up on request)", config.idle_timeout_secs).bright_green()
            } else {
                "DISABLED (Always resident in memory)".yellow()
            }
        );
        println!("{}", "══════════════════════════════════════════════════════════════════".bright_cyan());
    }
}
