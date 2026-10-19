//! Crash-recovery journal (H9 v2): `{uuid}.state` + `{uuid}.blobs/` + meta.
//!
//! Kill switch: `DITHER_JOURNAL=0` / `false` / `off`. Writes run on a dedicated
//! low-priority thread (not tokio / not tile workers). Legacy
//! `{uuid}.dyproj.journal` files are still readable (Step 4); new writes use
//! the state+blobs format only.

pub(crate) mod assemble;
pub(crate) mod cleanup;
pub mod clean_exit;
pub mod commands;
#[cfg(test)]
mod fail_tests;
#[cfg(test)]
pub(crate) mod failpoint;
#[cfg(test)]
mod premerge_tests;
mod fsutil;
mod meta;
pub mod roster;
mod runtime;
pub mod signals;
pub mod stats;
mod write;

pub use meta::{
    journal_blob_path, journal_blobs_dir, journal_state_path, recovery_subdir, JOURNAL_MAX_BYTES,
};
pub use runtime::{
    flush_doc_sync, schedule_dirty, set_recovery_dir, start_heartbeat, JournalRuntime, HEARTBEAT,
    MAX_INTERVAL, QUIET,
};
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
