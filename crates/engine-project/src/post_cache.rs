//! P2 feature flags for the Dithered / Post cache (`.local-doc/post.md`).
//!
//! On by default (A4). Set `DITHER_POST_CACHE=0` to disable Post-from-Dithered +
//! skipping residual clears on Post-only edits. `DITHER_POST_VERIFY=1` enables
//! cache-vs-full hash checks (dev).

use std::sync::atomic::{AtomicBool, Ordering};

static CACHE_ENABLED: AtomicBool = AtomicBool::new(true);
static VERIFY_ENABLED: AtomicBool = AtomicBool::new(false);
static NEUTRAL_VERIFY_ENABLED: AtomicBool = AtomicBool::new(false);
static INIT: AtomicBool = AtomicBool::new(false);

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// `DITHER_POST_CACHE`: default **on**. Explicit `0`/`false`/`no` disables.
fn post_cache_env_or_default() -> bool {
    match std::env::var("DITHER_POST_CACHE") {
        Ok(v) => match v.trim() {
            "0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" => false,
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" => true,
            _ => true,
        },
        Err(_) => true,
    }
}

/// Read env once (safe to call repeatedly). Call from app startup if desired.
pub fn init_from_env() {
    if INIT.swap(true, Ordering::Relaxed) {
        return;
    }
    let cache = post_cache_env_or_default();
    let verify = env_flag("DITHER_POST_VERIFY");
    let neutral_verify = env_flag("DITHER_NEUTRAL_VERIFY");
    CACHE_ENABLED.store(cache, Ordering::Relaxed);
    VERIFY_ENABLED.store(verify, Ordering::Relaxed);
    NEUTRAL_VERIFY_ENABLED.store(neutral_verify, Ordering::Relaxed);
    log::info!(
        target: "post_barrier",
        "DITHER_POST_CACHE={} (verify={})",
        if cache { "on" } else { "off" },
        verify
    );
}

/// Whether Post-from-Dithered + Post clear-skip is active.
pub fn post_cache_enabled() -> bool {
    if !INIT.load(Ordering::Relaxed) {
        init_from_env();
    }
    CACHE_ENABLED.load(Ordering::Relaxed)
}

/// Same rule as `filter_service::post_cache_refresh_policy` (for unit tests).
/// Returns `(clear_residuals, preserve_dithered)`.
pub fn refresh_policy_for_change(
    old: &[crate::filter::FilterInstance],
    new: &[crate::filter::FilterInstance],
    base_clear: bool,
) -> (bool, bool) {
    if !post_cache_enabled() {
        return (base_clear, false);
    }
    match crate::classify_change(old, new) {
        crate::ChangeClass::Post => (false, true),
        _ => (base_clear, false),
    }
}

/// Dev: compare Post-from-cache vs full-stack on tile complete.
pub fn post_verify_enabled() -> bool {
    if !INIT.load(Ordering::Relaxed) {
        init_from_env();
    }
    VERIFY_ENABLED.load(Ordering::Relaxed)
}

/// Dev: on neutral skip, also run the filter and assert bitwise equality with pass-through.
pub fn neutral_verify_enabled() -> bool {
    if !INIT.load(Ordering::Relaxed) {
        init_from_env();
    }
    NEUTRAL_VERIFY_ENABLED.load(Ordering::Relaxed)
}

/// Test / harness override (does not require env).
pub fn set_post_cache_enabled_for_test(on: bool) {
    INIT.store(true, Ordering::Relaxed);
    CACHE_ENABLED.store(on, Ordering::Relaxed);
}

pub fn set_post_verify_enabled_for_test(on: bool) {
    INIT.store(true, Ordering::Relaxed);
    VERIFY_ENABLED.store(on, Ordering::Relaxed);
}

pub fn set_neutral_verify_enabled_for_test(on: bool) {
    INIT.store(true, Ordering::Relaxed);
    NEUTRAL_VERIFY_ENABLED.store(on, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_callable_after_default_on() {
        // May already be init'd by other tests — ensure getters stay callable.
        let _ = post_cache_enabled();
        let _ = post_verify_enabled();
        // Static default before init is on (A4).
        assert!(CACHE_ENABLED.load(Ordering::Relaxed) || INIT.load(Ordering::Relaxed));
    }
}
