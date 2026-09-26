//! Crash-recovery journal: dirty docs → `{app_data}/recovery/{uuid}.dyproj.journal`.
//!
//! Phase 2 of JOURNAL_CRASH_RECOVERY_spec — debounce writes + delete on clean Save.
//! Startup recovery UI is Phase 3.

mod meta;
mod runtime;
mod write;
pub mod clean_exit;
pub mod commands;
pub mod roster;
pub mod signals;

pub use meta::recovery_subdir;
pub use runtime::{schedule_dirty, set_recovery_dir, start_heartbeat, JournalRuntime};
pub use write::{delete_journal, write_journal_for_doc};
