//! Panic must not cross the COM boundary.

use crate::diag_codes::{record, Reason};
use windows::core::{Error, Result as WinResult};
use windows::Win32::Foundation::E_FAIL;

pub fn catch_com<T>(f: impl FnOnce() -> WinResult<T>) -> WinResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            record(Reason::Internal);
            Err(Error::from(E_FAIL))
        }
    }
}
