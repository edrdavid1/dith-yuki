//! Prefer-GPU / force-CPU switches (env).

/// When set (`DITHER_FORCE_CPU=1`), never use GPU filters.
pub fn force_cpu() -> bool {
    env_flag_enabled(std::env::var("DITHER_FORCE_CPU").ok().as_deref())
}

/// When set (`DITHER_GPU=1`), prefer GPU for eligible filters when a context exists.
/// Default is off until D1 exit criteria are green (document in tasks §2.5).
pub fn prefer_gpu() -> bool {
    env_flag_enabled(std::env::var("DITHER_GPU").ok().as_deref())
}

/// Combined gate: GPU path may run only if not force-CPU and prefer-GPU is on.
pub fn gpu_filters_enabled() -> bool {
    !force_cpu() && prefer_gpu()
}

/// True for env values that opt into a boolean flag (`1` / `true` / `yes`).
fn env_flag_enabled(val: Option<&str>) -> bool {
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
}
