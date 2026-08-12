//! Prefer-GPU / force-CPU switches (env).

/// When set (`DITHER_FORCE_CPU=1`), never use GPU filters.
pub fn force_cpu() -> bool {
    matches!(
        std::env::var("DITHER_FORCE_CPU").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// When set (`DITHER_GPU=1`), prefer GPU for eligible filters when a context exists.
/// Default is off until D1 exit criteria are green (document in tasks §2.5).
pub fn prefer_gpu() -> bool {
    matches!(
        std::env::var("DITHER_GPU").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// Combined gate: GPU path may run only if not force-CPU and prefer-GPU is on.
pub fn gpu_filters_enabled() -> bool {
    !force_cpu() && prefer_gpu()
}
