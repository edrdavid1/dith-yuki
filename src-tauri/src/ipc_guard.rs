//! Panic boundary helpers for archive open/save IPC (SPEC §9.10).

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run `f` and map a panic to a user-facing error string.
///
/// Prefer this around archive parsers so a bug in the loader becomes an IPC
/// error instead of killing the app process. `spawn_blocking` JoinError is a
/// second line of defence when the work is already off-thread.
pub fn catch_loader_panic<T, E, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, E>,
    E: ToString,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(
            "Something went wrong while reading the file. The app stayed open — try again or update Dither."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ok_and_err() {
        assert_eq!(catch_loader_panic(|| Ok::<_, &str>(7)).unwrap(), 7);
        assert_eq!(
            catch_loader_panic(|| Err::<i32, _>("boom")).unwrap_err(),
            "boom"
        );
    }

    #[test]
    fn maps_panic_to_message() {
        let err = catch_loader_panic(|| -> Result<(), &str> {
            panic!("loader bug");
        })
        .unwrap_err();
        assert!(err.contains("Something went wrong"), "{err}");
    }
}
