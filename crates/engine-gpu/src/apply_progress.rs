//! Optional host hook for GPU strip progress (preview footer %).
//!
//! `engine-project` / Tauri install a listener; strip loops in blocked_fs /
//! blue_noise call [`report`] after each dispatch.

use std::sync::{Mutex, OnceLock};

type Listener = Box<dyn Fn(u32, u32) + Send + Sync>;

fn slot() -> &'static Mutex<Option<Listener>> {
    static S: OnceLock<Mutex<Option<Listener>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Install or clear the strip-progress listener (`done`, `total`).
pub fn set_listener(listener: Option<Listener>) {
    if let Ok(mut g) = slot().lock() {
        *g = listener;
    }
}

/// Report strip progress. No-op when no listener is installed.
pub fn report(done: u32, total: u32) {
    let total = total.max(1);
    let done = done.min(total);
    if let Ok(g) = slot().lock() {
        if let Some(ref f) = *g {
            f(done, total);
        }
    }
}
