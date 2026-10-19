//! Crash-recovery journal (H9 v2): `{uuid}.state` + `{uuid}.blobs/` + meta.
//!
//! Kill switch: `DITHER_JOURNAL=0` / `false` / `off`. Writes run on a dedicated
//! low-priority thread (not tokio / not tile workers). Legacy
//! `{uuid}.dyproj.journal` files are still readable (Step 4); new writes use
//! the state+blobs format only.

pub(crate) mod assemble;
pub mod clean_exit;
pub(crate) mod cleanup;
pub mod commands;
#[cfg(test)]
mod fail_tests;
#[cfg(test)]
pub(crate) mod failpoint;
mod fsutil;
mod meta;
#[cfg(test)]
mod premerge_tests;
pub mod roster;
mod runtime;
pub mod signals;
pub mod stats;
mod write;

pub use meta::{journal_state_path, recovery_subdir};
pub use runtime::{
    flush_doc_sync, schedule_dirty, set_recovery_dir, start_heartbeat, JournalRuntime,
};
pub use write::delete_journal;

/// Test / harness re-exports (`cargo clippy --bins` excludes `cfg(test)` call sites).
#[cfg(test)]
pub use meta::{journal_blob_path, journal_blobs_dir, JOURNAL_MAX_BYTES};
#[cfg(test)]
pub use runtime::{HEARTBEAT, MAX_INTERVAL, QUIET};
#[cfg(test)]
pub use stats::JournalWriteSource;
#[cfg(test)]
pub use write::{write_journal_for_doc, write_journal_for_doc_from};

/// `DITHER_JOURNAL=0` / `false` disables crash-recovery journal I/O (A/B).
#[allow(clippy::match_like_matches_macro)] // keep readable multi-predicate disable list
pub fn enabled() -> bool {
    match std::env::var("DITHER_JOURNAL") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off") => {
            false
        }
        _ => true,
    }
}
