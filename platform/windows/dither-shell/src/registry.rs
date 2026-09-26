//! Per-user (HKCU) registration helpers for the thumbnail provider.

use crate::{CLSID_THUMB_STR, PROGID_PATTERN, PROGID_PROJECT, SHELL_THUMB_HANDLER};

#[cfg(windows)]
mod win {
    use super::*;
    use std::path::PathBuf;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    use winreg::enums::*;
    use winreg::RegKey;

    fn module_path() -> Result<PathBuf, String> {
        let mut buf = [0u16; MAX_PATH as usize];
        let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
        if n == 0 {
            return Err("GetModuleFileNameW failed".into());
        }
        Ok(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }

    fn set_default(path: &str, value: &str) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(path).map_err(|e| e.to_string())?;
        key.set_value("", &value).map_err(|e| e.to_string())
    }

    fn set_named(path: &str, name: &str, value: &str) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(path).map_err(|e| e.to_string())?;
        key.set_value(name, &value).map_err(|e| e.to_string())
    }

    fn delete_tree(path: &str) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(path);
    }

    pub fn register_user(dll_override: Option<&str>) -> Result<(), String> {
        let dll = match dll_override {
            Some(p) => p.to_string(),
            None => module_path()?.to_string_lossy().into_owned(),
        };

        let clsid_key = format!("Software\\Classes\\CLSID\\{CLSID_THUMB_STR}");
        set_default(&clsid_key, "Dither Thumbnail Provider")?;
        set_default(&format!("{clsid_key}\\InprocServer32"), &dll)?;
        set_named(
            &format!("{clsid_key}\\InprocServer32"),
            "ThreadingModel",
            "Apartment",
        )?;

        for (ext, progid) in [(".dyproj", PROGID_PROJECT), (".dyuki", PROGID_PATTERN)] {
            set_default(
                &format!("Software\\Classes\\{ext}\\ShellEx\\{SHELL_THUMB_HANDLER}"),
                CLSID_THUMB_STR,
            )?;
            set_default(
                &format!("Software\\Classes\\{progid}\\ShellEx\\{SHELL_THUMB_HANDLER}"),
                CLSID_THUMB_STR,
            )?;
        }

        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
        Ok(())
    }

    pub fn unregister_user() -> Result<(), String> {
        delete_tree(&format!("Software\\Classes\\CLSID\\{CLSID_THUMB_STR}"));
        for ext in [".dyproj", ".dyuki"] {
            delete_tree(&format!(
                "Software\\Classes\\{ext}\\ShellEx\\{SHELL_THUMB_HANDLER}"
            ));
        }
        for progid in [PROGID_PROJECT, PROGID_PATTERN] {
            delete_tree(&format!(
                "Software\\Classes\\{progid}\\ShellEx\\{SHELL_THUMB_HANDLER}"
            ));
        }
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use win::{register_user, unregister_user};

#[cfg(not(windows))]
pub fn register_user(_dll_override: Option<&str>) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(not(windows))]
pub fn unregister_user() -> Result<(), String> {
    Err("windows only".into())
}
