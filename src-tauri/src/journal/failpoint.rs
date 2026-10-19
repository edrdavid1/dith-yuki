//! Test-only crash injection for journal write phases (H9 §6.1).
//!
//! Armed failpoints trip once then clear. Production builds compile the checks
//! away via `#[cfg(test)]` call sites.

#![cfg(test)]

use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPoint {
    /// After blob tmp is on disk, before rename into `.blobs/`.
    BeforeBlobRename,
    /// After a blob rename completed.
    AfterBlobWrite,
    /// Before writing `.state`.
    BeforeStateWrite,
    /// After `.state` rename completed.
    AfterStateWrite,
    /// Before writing `.meta.json`.
    BeforeMetaWrite,
    /// After meta write completed.
    AfterMetaWrite,
}

thread_local! {
    static ARMED: Cell<Option<FailPoint>> = const { Cell::new(None) };
}

pub fn arm(fp: FailPoint) {
    ARMED.with(|c| c.set(Some(fp)));
}

pub fn clear() {
    ARMED.with(|c| c.set(None));
}

/// Returns `Err` and clears the arm when `fp` matches.
pub fn check(fp: FailPoint) -> Result<(), String> {
    ARMED.with(|c| {
        if c.get() == Some(fp) {
            c.set(None);
            Err(format!("journal failpoint {fp:?}"))
        } else {
            Ok(())
        }
    })
}
