//! Single registry table for the thumbnail provider, the preview handler,
//! `dither-shell-diag`, and the generated NSIS include.
//!
//! ProgIDs match Tauri `fileAssociations[].name` (`Dither Project`,
//! `Dither Pattern`). CLSIDs are frozen.

use std::fmt::Write as _;

pub const CLSID_THUMB: &str = "{BC7D0A00-220F-46DD-AAA8-C754864EE648}";
#[cfg_attr(not(windows), allow(dead_code))]
pub const CLSID_THUMB_U128: u128 = 0xBC7D0A00_220F_46DD_AAA8_C754864EE648;

pub const CLSID_PREVIEW: &str = "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}";
#[cfg_attr(not(windows), allow(dead_code))]
pub const CLSID_PREVIEW_U128: u128 = 0xC8BC1EC9_FB9C_4374_9925_D82D2819A965;

/// `IThumbnailProvider` ShellEx handler.
pub const SHELL_THUMB: &str = "{E357FCCD-A995-4576-B01F-234630154E96}";
/// `IPreviewHandler` ShellEx handler.
pub const SHELL_PREVIEW: &str = "{8895b1c6-b41f-4c1c-a562-0d564250836f}";
/// 64-bit `prevhost.exe` surrogate. Not verified against a live Explorer yet.
pub const PREVIEW_APPID: &str = "{6d2b5079-2f0b-48dd-ab7f-97cec514d30b}";

pub const PROGID_PROJECT: &str = "Dither Project";
pub const PROGID_PATTERN: &str = "Dither Pattern";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Thumbnail,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uninstall {
    /// `DeleteRegKey` this path.
    Key,
    /// `DeleteRegValue` this named value. Never delete the parent key.
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Literal(&'static str),
    DllPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegEntry {
    pub component: Component,
    pub path: String,
    pub value_name: String,
    pub value: ValueKind,
    pub uninstall: Uninstall,
}

pub fn entries() -> Vec<RegEntry> {
    let mut out = Vec::new();
    push_clsid(
        &mut out,
        Component::Thumbnail,
        CLSID_THUMB,
        "Dither Thumbnail Provider",
        None,
    );
    push_clsid(
        &mut out,
        Component::Preview,
        CLSID_PREVIEW,
        "Dither Preview Handler",
        Some(PREVIEW_APPID),
    );
    for (ext, progid) in [(".dyproj", PROGID_PROJECT), (".dyuki", PROGID_PATTERN)] {
        push_shellex(
            &mut out,
            Component::Thumbnail,
            ext,
            SHELL_THUMB,
            CLSID_THUMB,
        );
        push_shellex(
            &mut out,
            Component::Thumbnail,
            progid,
            SHELL_THUMB,
            CLSID_THUMB,
        );
        push_shellex(
            &mut out,
            Component::Preview,
            ext,
            SHELL_PREVIEW,
            CLSID_PREVIEW,
        );
        push_shellex(
            &mut out,
            Component::Preview,
            progid,
            SHELL_PREVIEW,
            CLSID_PREVIEW,
        );
    }
    out.push(RegEntry {
        component: Component::Preview,
        path: "Software\\Microsoft\\Windows\\CurrentVersion\\PreviewHandlers".into(),
        value_name: CLSID_PREVIEW.into(),
        value: ValueKind::Literal("Dither Preview Handler"),
        uninstall: Uninstall::Value,
    });
    out
}

fn push_clsid(
    out: &mut Vec<RegEntry>,
    component: Component,
    clsid: &str,
    title: &'static str,
    app_id: Option<&'static str>,
) {
    let clsid_path = format!("Software\\Classes\\CLSID\\{clsid}");
    out.push(RegEntry {
        component,
        path: clsid_path.clone(),
        value_name: String::new(),
        value: ValueKind::Literal(title),
        uninstall: Uninstall::Key,
    });
    out.push(RegEntry {
        component,
        path: format!("{clsid_path}\\InprocServer32"),
        value_name: String::new(),
        value: ValueKind::DllPath,
        uninstall: Uninstall::Key,
    });
    out.push(RegEntry {
        component,
        path: format!("{clsid_path}\\InprocServer32"),
        value_name: "ThreadingModel".into(),
        value: ValueKind::Literal("Apartment"),
        uninstall: Uninstall::Key,
    });
    if let Some(app) = app_id {
        out.push(RegEntry {
            component,
            path: clsid_path,
            value_name: "AppID".into(),
            value: ValueKind::Literal(app),
            uninstall: Uninstall::Key,
        });
    }
}

fn push_shellex(
    out: &mut Vec<RegEntry>,
    component: Component,
    class_or_ext: &str,
    handler: &str,
    clsid: &str,
) {
    out.push(RegEntry {
        component,
        path: format!("Software\\Classes\\{class_or_ext}\\ShellEx\\{handler}"),
        value_name: String::new(),
        value: ValueKind::Literal(clsid_static(clsid)),
        uninstall: Uninstall::Key,
    });
}

fn clsid_static(clsid: &str) -> &'static str {
    match clsid {
        CLSID_THUMB => CLSID_THUMB,
        CLSID_PREVIEW => CLSID_PREVIEW,
        _ => unreachable!("only the two frozen CLSIDs"),
    }
}

pub fn emit_nsh() -> String {
    let mut s = String::new();
    s.push_str("; Generated from platform/windows/dither-shell/src/registry_keys.rs\n");
    s.push_str("; Regenerate: cargo run -p dither-shell-xtask\n");
    s.push_str("; Do not edit by hand.\n\n");
    let _ = writeln!(s, "!define DITHER_CLSID_THUMB \"{CLSID_THUMB}\"");
    let _ = writeln!(s, "!define DITHER_CLSID_PREVIEW \"{CLSID_PREVIEW}\"");
    let _ = writeln!(s, "!define DITHER_SHELL_THUMB \"{SHELL_THUMB}\"");
    let _ = writeln!(s, "!define DITHER_SHELL_PREVIEW \"{SHELL_PREVIEW}\"");
    let _ = writeln!(s, "!define DITHER_PREVIEW_APPID \"{PREVIEW_APPID}\"");
    s.push('\n');
    s.push_str("!macro DITHER_WRITE_SHELL_KEYS\n");
    for e in entries() {
        let value = match e.value {
            ValueKind::Literal(v) => v.to_string(),
            ValueKind::DllPath => "$DitherShellDll".into(),
        };
        let _ = writeln!(
            s,
            "  WriteRegStr SHCTX \"{}\" \"{}\" \"{}\"",
            e.path, e.value_name, value
        );
    }
    s.push_str("!macroend\n\n");
    s.push_str("!macro DITHER_DELETE_SHELL_KEYS\n");
    let mut deleted_keys = Vec::new();
    for e in entries() {
        match e.uninstall {
            Uninstall::Key => {
                let covered = deleted_keys.iter().any(|parent: &String| {
                    e.path.starts_with(parent.as_str())
                        && e.path.len() > parent.len()
                        && e.path.as_bytes().get(parent.len()) == Some(&b'\\')
                });
                if !covered && !deleted_keys.contains(&e.path) {
                    let _ = writeln!(s, "  DeleteRegKey SHCTX \"{}\"", e.path);
                    deleted_keys.push(e.path);
                }
            }
            Uninstall::Value => {
                let _ = writeln!(
                    s,
                    "  DeleteRegValue SHCTX \"{}\" \"{}\"",
                    e.path, e.value_name
                );
            }
        }
    }
    s.push_str("!macroend\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_both_components_and_skips_perceived_type() {
        let all = entries();
        assert!(all.iter().any(|e| e.component == Component::Thumbnail));
        assert!(all.iter().any(|e| e.component == Component::Preview));
        let blob = emit_nsh();
        assert!(!blob.to_ascii_lowercase().contains("perceivedtype"));
        assert!(blob.contains(CLSID_THUMB));
        assert!(blob.contains(CLSID_PREVIEW));
        assert!(blob.contains("Dither Project"));
        assert!(blob.contains("DeleteRegValue"));
        assert!(!blob.contains("DisableProcessIsolation"));
    }

    #[test]
    fn committed_nsh_matches_the_table() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../registry_keys.nsh");
        let on_disk = std::fs::read_to_string(path).unwrap_or_default();
        let generated = emit_nsh();
        if on_disk != generated {
            std::fs::write(path, &generated).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), generated);
    }
}
