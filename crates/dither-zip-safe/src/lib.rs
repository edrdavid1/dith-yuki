//! Shared secure ZIP primitives (preview SPEC §5).
//!
//! `#![forbid(unsafe_code)]`. No engine / Tauri dependencies.
//! Used by `engine-project` (loader) and `dither-thumb` (OS previews).

#![forbid(unsafe_code)]

pub mod entry;
pub mod io;
pub mod limits;
pub mod limited_reader;

pub use entry::{
    is_allowlisted_entry, validate_entry_syntax, EntryClass, EntrySyntaxError, MAX_ENTRY_NAME_BYTES,
    MAX_PATH_SEGMENTS,
};
pub use io::{IoError, ReadAt};
pub use limits::{ArchiveLimits, ThumbLimits};
pub use limited_reader::LimitedReader;
