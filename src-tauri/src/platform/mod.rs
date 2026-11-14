//! OS-specific helpers that are independent of distribution channel.
//! Channel logic lives only in `crate::channel`.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod macos;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod windows;
