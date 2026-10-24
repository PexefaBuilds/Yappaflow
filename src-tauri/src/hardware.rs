//! What this PC can run locally: GPU vendor + dedicated VRAM (DXGI), RAM and cores.
//! Exception to the "OS code lives in four places" rule (CLAUDE.md): hardware detection
//! is new and OS-specific, so the DXGI call sits in one small `#[cfg(windows)]` block below.
//! Free VRAM via NVML is skipped: it needs the nvml-wrapper crate; dedicated VRAM is enough
//! for the recommendation table.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Hardware {
    /// Adapter name as the driver reports it, e.g. "NVIDIA GeForce RTX 2070 SUPER". Empty = none found.
    pub gpu_name: String,
    /// nvidia | amd | intel | apple | "" (unknown / none)
    pub gpu_vendor: String,
    pub vram_mb: u64,
    pub ram_mb: u64,
    pub cores: usize,
}

#[cfg_attr(not(windows), allow(dead_code))] // only the Windows GPU probe reads PCI ids
pub fn vendor(pci_id: u32) -> &'static str {
    match pci_id {
        0x10DE => "nvidia",
        0x1002 => "amd",
        0x8086 => "intel",
        _ => "",
    }
}

pub fn detect() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let (gpu_name, gpu_vendor, vram_mb) = gpu().unwrap_or_default();
    Hardware {
        gpu_name,
        gpu_vendor,
        vram_mb,
        ram_mb: sys.total_memory() / (1024 * 1024),
        cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    }
}

/// The hardware adapter with the most dedicated VRAM (skips the software renderer).
#[cfg(windows)]
fn gpu() -> Option<(String, String, u64)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best: Option<(String, String, u64)> = None;
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let vram = d.DedicatedVideoMemory as u64 / (1024 * 1024);
        if best.as_ref().map_or(true, |b| vram > b.2) {
            let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
            best = Some((String::from_utf16_lossy(&d.Description[..len]).trim().to_string(), vendor(d.VendorId).to_string(), vram));
        }
    }
    best
}

/// Apple Silicon shares RAM with the GPU (Phase 8 refines this).
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn gpu() -> Option<(String, String, u64)> {
    Some(("Apple Silicon".into(), "apple".into(), 0))
}

#[cfg(not(any(windows, all(target_os = "macos", target_arch = "aarch64"))))]
fn gpu() -> Option<(String, String, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_ids() {
        assert_eq!((vendor(0x10DE), vendor(0x1002), vendor(0x8086), vendor(0x1414)), ("nvidia", "amd", "intel", ""));
    }

    #[test]
    fn detect_does_not_panic() {
        let hw = detect();
        assert!(hw.cores >= 1 && hw.ram_mb > 0, "{hw:?}");
    }
}
