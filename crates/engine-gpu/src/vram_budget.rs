//! Track C Phase 1: VRAM resident-cache budget (once at adapter init).

use crate::resident::DEFAULT_VRAM_BUDGET_BYTES;

/// Floor: current 256 MiB default.
pub const MIN_VRAM_BUDGET_BYTES: u64 = DEFAULT_VRAM_BUDGET_BYTES;
/// Cap: slot count is still limited by `max_texture_array_layers`.
pub const MAX_VRAM_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramBudgetSource {
    Adaptive,
    MinFallback,
    EnvOverride,
}

impl VramBudgetSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::MinFallback => "min_fallback",
            Self::EnvOverride => "env_override",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramBudget {
    pub bytes: u64,
    pub source: VramBudgetSource,
}

/// 25% of reported adapter memory, clamped. `None` / junk → MIN (do not guess).
pub fn compute_vram_budget(available_vram: Option<u64>) -> VramBudget {
    let Some(available) = available_vram.filter(|&n| n >= MIN_VRAM_BUDGET_BYTES) else {
        return VramBudget {
            bytes: MIN_VRAM_BUDGET_BYTES,
            source: VramBudgetSource::MinFallback,
        };
    };
    let share = available / 4;
    if share <= MIN_VRAM_BUDGET_BYTES {
        return VramBudget {
            bytes: MIN_VRAM_BUDGET_BYTES,
            source: VramBudgetSource::MinFallback,
        };
    }
    VramBudget {
        bytes: share.min(MAX_VRAM_BUDGET_BYTES),
        source: VramBudgetSource::Adaptive,
    }
}

pub fn resolve_vram_budget(available_vram: Option<u64>) -> VramBudget {
    if let Some(over) = env_vram_override() {
        return over;
    }
    compute_vram_budget(available_vram)
}

fn env_vram_override() -> Option<VramBudget> {
    let raw = std::env::var("DITHER_VRAM_BUDGET_MIB").ok()?;
    let mib: u64 = raw.parse().ok()?;
    if mib == 0 {
        return None;
    }
    Some(VramBudget {
        bytes: mib.saturating_mul(1024 * 1024),
        source: VramBudgetSource::EnvOverride,
    })
}

/// Only returns a number when the backend can give a plausible working-set size.
pub fn query_adapter_memory(adapter: &wgpu::Adapter) -> Option<u64> {
    match adapter.get_info().backend {
        wgpu::Backend::Metal => query_metal_working_set_bytes(),
        _ => None,
    }
}

fn query_metal_working_set_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        macos_metal_recommended_working_set()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // `objc` msg_send! probes `cfg(cargo_clippy)` as a feature.
fn macos_metal_recommended_working_set() -> Option<u64> {
    use objc::{msg_send, runtime::Object, sel, sel_impl};

    #[link(name = "Metal", kind = "framework")]
    extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut Object;
    }

    unsafe {
        let device = MTLCreateSystemDefaultDevice();
        if device.is_null() {
            return None;
        }
        let size: u64 = msg_send![device, recommendedMaxWorkingSetSize];
        let _: () = msg_send![device, release];
        if size < MIN_VRAM_BUDGET_BYTES {
            None
        } else {
            Some(size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_falls_back_to_min() {
        let b = compute_vram_budget(None);
        assert_eq!(b.bytes, MIN_VRAM_BUDGET_BYTES);
        assert_eq!(b.source, VramBudgetSource::MinFallback);
    }

    #[test]
    fn tiny_report_is_unreliable() {
        let b = compute_vram_budget(Some(64 * 1024 * 1024));
        assert_eq!(b.bytes, MIN_VRAM_BUDGET_BYTES);
        assert_eq!(b.source, VramBudgetSource::MinFallback);
    }

    #[test]
    fn eight_gib_quarter_clamps_to_max() {
        let b = compute_vram_budget(Some(8 * 1024 * 1024 * 1024));
        assert_eq!(b.bytes, MAX_VRAM_BUDGET_BYTES);
        assert_eq!(b.source, VramBudgetSource::Adaptive);
    }

    #[test]
    fn two_gib_is_quarter() {
        let b = compute_vram_budget(Some(2 * 1024 * 1024 * 1024));
        assert_eq!(b.bytes, 512 * 1024 * 1024);
        assert_eq!(b.source, VramBudgetSource::Adaptive);
    }
}
