//! Crash-recovery journal: dirty docs → `{app_data}/recovery/{uuid}.dyproj.journal`.
//!
//! Phase 2 of JOURNAL_CRASH_RECOVERY_spec — debounce writes + delete on clean Save.
//! Startup recovery UI is Phase 3.
//!
//! Kill switch for A/B latency tests: `DITHER_JOURNAL=0` (or `false`) skips
//! recovery-dir setup, debounce writes, and the heartbeat. Journal writes call
//! [`save_project_to_bytes`](engine_project::serialize::save_project_to_bytes),
//! which rebuilds a **processed** composite (re-applies dither) — that can
//! contend with the tile worker during live dithering.

pub mod clean_exit;
pub mod commands;
mod meta;
pub mod roster;
mod runtime;
pub mod signals;
pub mod stats;
mod write;

pub use meta::{journal_blob_path, recovery_subdir, JOURNAL_MAX_BYTES};
pub use runtime::{schedule_dirty, set_recovery_dir, start_heartbeat, JournalRuntime};
pub use stats::JournalWriteSource;
pub use write::{delete_journal, write_journal_for_doc, write_journal_for_doc_from};

/// `DITHER_JOURNAL=0` / `false` disables crash-recovery journal I/O (A/B).
pub fn enabled() -> bool {
    match std::env::var("DITHER_JOURNAL") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off") => {
            false
        }
        _ => true,
    }
}
