//! Windows shell thumbnail provider for `.dyproj` / `.dyuki`.
//!
//! CLSID is frozen forever — see `CLSID_THUMB` and `docs/PREVIEWS.md`.

#![cfg_attr(windows, windows_subsystem = "windows")]

/// Frozen CLSID for the thumbnail provider (preview SPEC §7.1).
/// `{BC7D0A00-220F-46DD-AAA8-C754864EE648}`
pub const CLSID_THUMB_STR: &str = "{BC7D0A00-220F-46DD-AAA8-C754864EE648}";

/// ShellEx key for `IThumbnailProvider`.
pub const SHELL_THUMB_HANDLER: &str = "{E357FCCD-A995-4576-B01F-234630154E96}";

/// ProgIDs as registered by Tauri `fileAssociations[].name`.
pub const PROGID_PROJECT: &str = "Dither Project";
pub const PROGID_PATTERN: &str = "Dither Pattern";

#[cfg(windows)]
mod bitmap;
#[cfg(windows)]
mod factory;
#[cfg(windows)]
mod provider;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod stream_io;

#[cfg(windows)]
pub use registry::{register_user, unregister_user};

/// Re-export COM entry points on Windows.
#[cfg(windows)]
pub use factory::exports::{
    DllCanUnloadNow, DllGetClassObject, DllRegisterServer, DllUnregisterServer,
};

#[cfg(not(windows))]
/// Stub so the crate compiles on non-Windows hosts (macOS/Linux CI).
pub fn windows_only_notice() -> &'static str {
    "dither-shell COM DLL builds only on Windows targets"
}

#[cfg(test)]
mod tests {
    #[test]
    fn clsid_format() {
        assert_eq!(
            super::CLSID_THUMB_STR,
            "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
        );
    }
}
