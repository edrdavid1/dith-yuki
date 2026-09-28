//! Windows shell thumbnail provider and preview handler for `.dyproj` / `.dyuki`.
//!
//! CLSIDs are frozen in `registry_keys`. Geometry comes from `dither-thumb`.

mod color;
mod diag_codes;
mod extract;
mod file_io;
mod geometry;
mod pe;
mod png_out;
mod registry_keys;
mod render;

#[cfg(windows)]
mod factory;
#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod guard;
#[cfg(windows)]
mod preview;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod stream_io;
#[cfg(windows)]
mod thumbnail;

#[cfg(not(windows))]
mod registry;

pub use color::{rgba_premul_to_bgra, unpremultiply_rgba};
pub use diag_codes::{recent_lines, Reason};
pub use geometry::{composite_centered, fit_centered, opaque_bounds};
pub use registry::{check_lines, register, unregister, Hive};
pub use registry_keys::{emit_nsh, CLSID_PREVIEW, CLSID_THUMB, PROGID_PATTERN, PROGID_PROJECT};
pub use render::{render_file, render_text};

#[cfg(windows)]
pub use factory::exports::{
    DllCanUnloadNow, DllGetClassObject, DllRegisterServer, DllUnregisterServer,
};

#[cfg(windows)]
pub use preview::post_quit;

/// Clear Explorer thumbnail and icon caches, then restart Explorer.
#[cfg(windows)]
pub fn clear_cache() -> Result<(), String> {
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "explorer.exe"])
        .status();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = std::path::Path::new(&local)
            .join("Microsoft")
            .join("Windows")
            .join("Explorer");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("thumbcache_") || name.starts_with("iconcache") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    let _ = std::process::Command::new("explorer.exe").spawn();
    unsafe {
        windows::Win32::UI::Shell::SHChangeNotify(
            windows::Win32::UI::Shell::SHCNE_ASSOCCHANGED,
            windows::Win32::UI::Shell::SHCNF_IDLIST,
            None,
            None,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn clear_cache() -> Result<(), String> {
    Err("clear-cache runs on Windows".into())
}

/// `IShellItemImageFactory` with `SIIGBF_THUMBNAILONLY`.
#[cfg(windows)]
pub fn shell_thumb_rgba(path: &std::path::Path, size: u32) -> Result<(u32, u32, Vec<u8>), String> {
    use crate::gdi::{delete_bitmap, read_premul_rgba};
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(windows::core::PCWSTR(wide.as_ptr()), None)
                .map_err(|e| e.to_string())?;
        let hbmp = factory
            .GetImage(
                SIZE {
                    cx: size as i32,
                    cy: size as i32,
                },
                SIIGBF_THUMBNAILONLY,
            )
            .map_err(|e| e.to_string())?;
        let read = read_premul_rgba(hbmp).map_err(|e| e.to_string());
        delete_bitmap(hbmp);
        read
    }
}

#[cfg(not(windows))]
pub fn shell_thumb_rgba(
    _path: &std::path::Path,
    _size: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    Err("shell-thumb runs on Windows".into())
}
