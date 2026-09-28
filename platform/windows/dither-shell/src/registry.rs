//! Register, unregister, and check the keys from [`crate::registry_keys`].

use crate::registry_keys::{self, Component};

#[derive(Clone, Copy)]
pub enum Hive {
    User,
    Machine,
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::pe::{machine_name, pe_machine, MACHINE_AMD64, MACHINE_ARM64};
    use crate::registry_keys::{Uninstall, ValueKind};
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    use winreg::enums::*;
    use winreg::RegKey;

    fn root(hive: Hive) -> RegKey {
        match hive {
            Hive::User => RegKey::predef(HKEY_CURRENT_USER),
            Hive::Machine => RegKey::predef(HKEY_LOCAL_MACHINE),
        }
    }

    fn module_path() -> Result<PathBuf, String> {
        let mut buf = [0u16; MAX_PATH as usize];
        let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
        if n == 0 {
            return Err("GetModuleFileNameW failed".into());
        }
        Ok(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }

    fn notify() {
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
    }

    pub fn register(hive: Hive, dll_override: Option<&str>) -> Result<(), String> {
        let dll = match dll_override {
            Some(p) => p.to_string(),
            None => module_path()?.to_string_lossy().into_owned(),
        };
        let hk = root(hive);
        for entry in registry_keys::entries() {
            let (key, _) = hk.create_subkey(&entry.path).map_err(|e| e.to_string())?;
            let value = match entry.value {
                ValueKind::Literal(v) => v.to_string(),
                ValueKind::DllPath => dll.clone(),
            };
            key.set_value(&entry.value_name, &value)
                .map_err(|e| e.to_string())?;
        }
        notify();
        Ok(())
    }

    pub fn unregister(hive: Hive) -> Result<(), String> {
        let hk = root(hive);
        let mut deleted = Vec::new();
        for entry in registry_keys::entries() {
            match entry.uninstall {
                Uninstall::Key => {
                    if !deleted.contains(&entry.path) {
                        let _ = hk.delete_subkey_all(&entry.path);
                        deleted.push(entry.path);
                    }
                }
                Uninstall::Value => {
                    if let Ok(key) = hk.open_subkey_with_flags(&entry.path, KEY_SET_VALUE) {
                        let _ = key.delete_value(&entry.value_name);
                    }
                }
            }
        }
        notify();
        Ok(())
    }

    pub fn check_lines(hive: Hive) -> Vec<String> {
        let hk = root(hive);
        let mut lines = Vec::new();
        let mut dll_path: Option<String> = None;
        for component in [Component::Thumbnail, Component::Preview] {
            let label = match component {
                Component::Thumbnail => "thumbnail",
                Component::Preview => "preview",
            };
            for entry in registry_keys::entries()
                .into_iter()
                .filter(|e| e.component == component)
            {
                let name = if entry.value_name.is_empty() {
                    "(default)".to_string()
                } else {
                    entry.value_name.clone()
                };
                match hk.open_subkey(&entry.path) {
                    Ok(key) => match key.get_value::<String, _>(&entry.value_name) {
                        Ok(value) => {
                            if matches!(entry.value, ValueKind::DllPath) {
                                dll_path = Some(value.clone());
                            }
                            lines.push(format!("{label} key {} {name} PASS", entry.path));
                        }
                        Err(_) => lines.push(format!(
                            "{label} key {} {name} FAIL missing value",
                            entry.path
                        )),
                    },
                    Err(_) => lines.push(format!("{label} key {} FAIL missing", entry.path)),
                }
            }
            match dll_path.clone() {
                Some(path) => {
                    if Path::new(&path).is_file() {
                        lines.push(format!("{label} dll.path PASS"));
                        lines.push(arch_line(label, Path::new(&path)));
                    } else {
                        lines.push(format!("{label} dll.path FAIL missing"));
                    }
                }
                None => lines.push(format!("{label} dll.path FAIL not registered")),
            }
        }
        lines
    }

    fn arch_line(label: &str, path: &Path) -> String {
        let Ok(bytes) = std::fs::read(path) else {
            return format!("{label} dll.arch FAIL unreadable");
        };
        let Some(machine) = pe_machine(&bytes) else {
            return format!("{label} dll.arch FAIL not a PE");
        };
        let mut process = Default::default();
        let mut native = Default::default();
        let os = unsafe {
            if IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)).is_ok() {
                native.0
            } else {
                0
            }
        };
        let os_name = machine_name(os);
        let dll_name = machine_name(machine);
        if os == machine || (os == 0 && matches!(machine, MACHINE_AMD64 | MACHINE_ARM64)) {
            format!("{label} dll.arch PASS {dll_name}")
        } else if os != 0 && os != machine {
            format!("{label} dll.arch FAIL ARCH_MISMATCH dll={dll_name} os={os_name}")
        } else {
            format!("{label} dll.arch PASS {dll_name}")
        }
    }
}

#[cfg(windows)]
pub use win::{check_lines, register, unregister};

#[cfg(not(windows))]
pub fn register(_hive: Hive, _dll_override: Option<&str>) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(not(windows))]
pub fn unregister(_hive: Hive) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(not(windows))]
pub fn check_lines(_hive: Hive) -> Vec<String> {
    registry_keys::entries()
        .into_iter()
        .map(|e| {
            let label = match e.component {
                Component::Thumbnail => "thumbnail",
                Component::Preview => "preview",
            };
            format!("{label} key {} SKIP (not Windows)", e.path)
        })
        .collect()
}
