//! macOS sandbox file access.
//!
//! A path string from the open panel is not enough after relaunch. Store
//! builds keep a security-scoped bookmark and start access before reading.

use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};

/// `NSURLBookmarkCreationWithSecurityScope`
const CREATE_SECURITY_SCOPE: usize = 1 << 11;
/// `NSURLBookmarkResolutionWithSecurityScope`
const RESOLVE_SECURITY_SCOPE: usize = 1 << 10;

pub struct BookmarkGuard {
    url: id,
}

impl Drop for BookmarkGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.url.is_null() {
                let _: bool = msg_send![self.url, stopAccessingSecurityScopedResource];
                let _: () = msg_send![self.url, release];
            }
        }
    }
}

pub fn empty_guard() -> BookmarkGuard {
    BookmarkGuard { url: nil }
}

/// Bookmark bytes (base64) for `path`, or `None` when the sandbox will not grant one.
pub fn create_security_bookmark(path: &str) -> Option<String> {
    unsafe {
        let ns_path: id = NSString::alloc(nil).init_str(path);
        let url: id = msg_send![class!(NSURL), fileURLWithPath: ns_path];
        if url.is_null() {
            return None;
        }
        let mut err: id = nil;
        let data: id = msg_send![url, bookmarkDataWithOptions: CREATE_SECURITY_SCOPE
            includingResourceValuesForKeys: nil
            relativeToURL: nil
            error: &mut err];
        if data.is_null() {
            log::info!("security-scoped bookmark unavailable for {path}");
            return None;
        }
        let len: usize = msg_send![data, length];
        let ptr: *const u8 = msg_send![data, bytes];
        if ptr.is_null() || len == 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        Some(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }
}

pub fn begin_bookmark_access(bookmark_b64: Option<&str>) -> BookmarkGuard {
    let Some(b64) = bookmark_b64 else {
        return empty_guard();
    };
    let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64) else {
        return empty_guard();
    };
    unsafe {
        let data: id = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() length: bytes.len()];
        if data.is_null() {
            return empty_guard();
        }
        let mut stale: i8 = 0;
        let mut err: id = nil;
        let url: id = msg_send![class!(NSURL), URLByResolvingBookmarkData: data
            options: RESOLVE_SECURITY_SCOPE
            relativeToURL: nil
            bookmarkDataIsStale: &mut stale
            error: &mut err];
        if url.is_null() {
            return empty_guard();
        }
        let started: i8 = msg_send![url, startAccessingSecurityScopedResource];
        if started == 0 {
            return empty_guard();
        }
        let url: id = msg_send![url, retain];
        BookmarkGuard { url }
    }
}

impl BookmarkGuard {
    pub fn is_active(&self) -> bool {
        !self.url.is_null()
    }
}

pub fn bookmark_still_valid(bookmark_b64: &str) -> bool {
    begin_bookmark_access(Some(bookmark_b64)).is_active()
}

// The guard is retained only to call start/stop. Both are process-wide and
// may run off the thread that created the NSURL.
unsafe impl Send for BookmarkGuard {}
