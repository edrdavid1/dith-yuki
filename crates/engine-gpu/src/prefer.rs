//! Prefer-GPU / force-CPU switches (env + optional UI override).

use std::sync::atomic::{AtomicU8, Ordering};

/// Test/diag override for cold GPU compute (`0` = unset, `1` = off, `2` = on).
/// Used when `DITHER_GPU_PREVIEW` env is **not** set (env wins for soak/CI).
static GPU_PREVIEW_UI_OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// When set (`DITHER_FORCE_CPU=1`), never use GPU filters.
pub fn force_cpu() -> bool {
    env_flag_enabled(std::env::var("DITHER_FORCE_CPU").ok().as_deref())
}

/// Prefer GPU for eligible filters (retired legacy `DITHER_GPU` semantics; maps to `gpu_preview_enabled`).
pub fn prefer_gpu() -> bool {
    gpu_preview_enabled()
}

/// Combined gate for **cold** GPU filter compute (opt-in). Warm download uses [`crate::decide_tile_dispatch`].
pub fn gpu_filters_enabled() -> bool {
    gpu_preview_enabled()
}

/// Speculative VRAM warm-up (A2). Default **on** when GPU exists (caller checks adapter).
///
/// `DITHER_GPU_WARMUP=0` disables. `DITHER_FORCE_CPU` wins.
pub fn gpu_warmup_enabled() -> bool {
    if force_cpu() {
        return false;
    }
    if let Ok(v) = std::env::var("DITHER_GPU_WARMUP") {
        return gpu_flag_enabled(Some(v.as_str()));
    }
    true
}

/// Path B GPU-resident frame path (opt-in compute, A2 warm-up, or `DITHER_GPU_RESIDENT`).
pub fn gpu_resident_enabled() -> bool {
    gpu_preview_enabled()
        || gpu_warmup_enabled()
        || gpu_flag_enabled(std::env::var("DITHER_GPU_RESIDENT").ok().as_deref())
}

/// Set test/diag override for cold GPU compute (no product UI).
///
/// `None` clears the override (fall through to env / compile-time).
/// Ignored while `DITHER_GPU_PREVIEW` env is set — env wins for soak/CI.
pub fn set_gpu_preview_ui_override(enabled: Option<bool>) {
    let v = match enabled {
        None => 0u8,
        Some(false) => 1,
        Some(true) => 2,
    };
    GPU_PREVIEW_UI_OVERRIDE.store(v, Ordering::Relaxed);
}

/// Opt-in for **cold** GPU compute (promote + graph) on eligible L0 tiles.
///
/// Warm Composite slots may still be downloaded when this is false (A1 auto-dispatch).
/// Precedence: `DITHER_FORCE_CPU` → off; else explicit `DITHER_GPU_PREVIEW` env;
/// else test/diag override; else compile-time `DITHER_GPU_PREVIEW`; else **off**.
pub fn gpu_preview_enabled() -> bool {
    if force_cpu() {
        return false;
    }
    if let Ok(v) = std::env::var("DITHER_GPU_PREVIEW") {
        return gpu_flag_enabled(Some(v.as_str()));
    }
    match GPU_PREVIEW_UI_OVERRIDE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => gpu_flag_enabled(option_env!("DITHER_GPU_PREVIEW")),
    }
}

/// True for env values that opt into a boolean flag (`1` / `true` / `yes`).
fn env_flag_enabled(val: Option<&str>) -> bool {
    gpu_flag_enabled(val)
}

fn gpu_flag_enabled(val: Option<&str>) -> bool {
    matches!(val, Some("1") | Some("true") | Some("TRUE") | Some("yes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dither_gpu_default_is_cpu() {
        assert!(!env_flag_enabled(None));
        assert!(!env_flag_enabled(Some("")));
        assert!(!env_flag_enabled(Some("0")));
        assert!(!env_flag_enabled(Some("false")));
        assert!(env_flag_enabled(Some("1")));
        assert!(env_flag_enabled(Some("true")));
    }

    #[test]
    fn ui_override_applies_when_env_unset() {
        // Isolate from leftover env in the process (tests share process).
        let had = std::env::var("DITHER_GPU_PREVIEW").ok();
        std::env::remove_var("DITHER_GPU_PREVIEW");
        set_gpu_preview_ui_override(None);
        let before = gpu_preview_enabled();
        set_gpu_preview_ui_override(Some(true));
        assert!(gpu_preview_enabled() || force_cpu());
        set_gpu_preview_ui_override(Some(false));
        if !force_cpu() {
            assert!(!gpu_preview_enabled());
        }
        set_gpu_preview_ui_override(None);
        assert_eq!(gpu_preview_enabled(), before);
        match had {
            Some(v) => std::env::set_var("DITHER_GPU_PREVIEW", v),
            None => {}
        }
    }
}
