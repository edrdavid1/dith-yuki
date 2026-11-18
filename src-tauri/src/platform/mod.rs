//! OS-specific helpers that are independent of distribution channel.
//! Channel logic lives only in `crate::channel`.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{begin_bookmark_access, bookmark_still_valid, create_security_bookmark, BookmarkGuard};

#[cfg(not(target_os = "macos"))]
pub struct BookmarkGuard;

#[cfg(not(target_os = "macos"))]
impl BookmarkGuard {
    pub fn is_active(&self) -> bool {
        false
    }
}

#[cfg(not(target_os = "macos"))]
impl Drop for BookmarkGuard {
    fn drop(&mut self) {}
}

#[cfg(not(target_os = "macos"))]
pub fn create_security_bookmark(_path: &str) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn begin_bookmark_access(_bookmark_b64: Option<&str>) -> BookmarkGuard {
    BookmarkGuard
}

#[cfg(not(target_os = "macos"))]
pub fn bookmark_still_valid(_bookmark_b64: &str) -> bool {
    false
}
