//! Sandbox path validation for secure file access.
//!
//! Ensures user-supplied file paths are confined within the home directory
//! and match allowed file extensions before any I/O operations proceed.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during sandbox path validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxError {
    /// The file extension is not in the allowed list.
    #[error("file extension not allowed")]
    BadExtension,

    /// The resolved path escapes the user's home directory.
    #[error("path resolves outside home directory")]
    OutsideHome,

    /// The file does not exist on disk.
    #[error("file not found")]
    NotFound,

    /// Could not determine the user's home directory.
    #[error("unable to determine home directory")]
    NoHome,
}

/// Trim + strip Windows `\\?\` / `\\.\` prefixes and trailing dots/spaces that
/// break `Path::extension()` on some dialog results.
pub fn normalize_path_input(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for prefix in ["\\\\?\\", "//?/", "\\\\.\\"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    while s.ends_with([' ', '.']) {
        s.pop();
    }
    s
}

/// If `raw` has no extension matching `default_ext`, append `.$default_ext`.
pub fn ensure_extension(raw: &str, default_ext: &str) -> String {
    let normalized = normalize_path_input(raw);
    let path = Path::new(&normalized);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case(default_ext) {
        return normalized;
    }
    if ext.is_empty() {
        let trimmed = normalized.trim_end_matches('.');
        return format!("{trimmed}.{default_ext}");
    }
    normalized
}

fn extension_allowed(path: &Path, allowed_ext: &[&str]) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    allowed_ext
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
}

/// Validates and resolves a user-supplied path, ensuring it:
/// - Has an extension present in `allowed_ext` (case-insensitive)
/// - Resolves to a location within the user's home directory
/// - Actually exists on disk
pub fn resolve_user_path(raw: &str, allowed_ext: &[&str]) -> Result<PathBuf, SandboxError> {
    let normalized = normalize_path_input(raw);
    let path = Path::new(&normalized);

    if !extension_allowed(path, allowed_ext) {
        return Err(SandboxError::BadExtension);
    }

    let canonical = path.canonicalize().map_err(|_| SandboxError::NotFound)?;

    let home = dirs::home_dir().ok_or(SandboxError::NoHome)?;
    if !canonical.starts_with(&home) {
        return Err(SandboxError::OutsideHome);
    }

    Ok(canonical)
}

/// Validates a path for **writing** a new file:
/// - Extension must be in `allowed_ext` (callers should [`ensure_extension`] first)
/// - Parent directory must exist and resolve under the user's home directory
/// - The file itself need not exist yet
pub fn resolve_export_path(raw: &str, allowed_ext: &[&str]) -> Result<PathBuf, SandboxError> {
    let normalized = normalize_path_input(raw);
    let path = Path::new(&normalized);

    if !extension_allowed(path, allowed_ext) {
        return Err(SandboxError::BadExtension);
    }

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = parent.canonicalize().map_err(|_| SandboxError::NotFound)?;

    let home = dirs::home_dir().ok_or(SandboxError::NoHome)?;
    if !canonical_parent.starts_with(&home) {
        return Err(SandboxError::OutsideHome);
    }

    let file_name = path.file_name().ok_or(SandboxError::BadExtension)?;
    Ok(canonical_parent.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_temp_file(subdir: &str, name: &str) -> PathBuf {
        let home = dirs::home_dir().expect("need home dir for tests");
        let dir = home.join(".dither_yuki_test_sandbox").join(subdir);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join(name);
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(b"test").unwrap();
        file_path
    }

    fn cleanup_subdir(subdir: &str) {
        let home = dirs::home_dir().expect("need home dir for tests");
        let dir = home.join(".dither_yuki_test_sandbox").join(subdir);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_path_with_matching_extension() {
        let file = create_temp_file("valid_ext", "test_image.png");
        let result = resolve_user_path(file.to_str().unwrap(), &["png", "jpg"]);
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("test_image.png"));
        cleanup_subdir("valid_ext");
    }

    #[test]
    fn valid_path_extension_case_insensitive() {
        let file = create_temp_file("case_insensitive", "test_image.PNG");
        let result = resolve_user_path(file.to_str().unwrap(), &["png"]);
        assert!(result.is_ok());
        cleanup_subdir("case_insensitive");
    }

    #[test]
    fn wrong_extension_returns_bad_extension() {
        let file = create_temp_file("wrong_ext", "test_image.bmp");
        let result = resolve_user_path(file.to_str().unwrap(), &["png", "jpg"]);
        assert_eq!(result, Err(SandboxError::BadExtension));
        cleanup_subdir("wrong_ext");
    }

    #[test]
    fn no_extension_returns_bad_extension() {
        let result = resolve_user_path("/some/path/noext", &["png"]);
        assert_eq!(result, Err(SandboxError::BadExtension));
    }

    #[test]
    fn ensure_extension_appends_when_missing() {
        assert_eq!(
            ensure_extension(r"C:\Users\a\proj", "dyproj"),
            r"C:\Users\a\proj.dyproj"
        );
        assert_eq!(
            ensure_extension(r"C:\Users\a\proj.dyproj", "dyproj"),
            r"C:\Users\a\proj.dyproj"
        );
        assert_eq!(
            ensure_extension(r"C:\Users\a\proj.DYPROJ ", "dyproj"),
            r"C:\Users\a\proj.DYPROJ"
        );
    }

    #[test]
    fn normalize_strips_verbatim_prefix() {
        let n = normalize_path_input(r"\\?\C:\Users\a\x.dyproj");
        assert_eq!(n, r"C:\Users\a\x.dyproj");
    }

    #[test]
    fn non_existent_file_returns_not_found() {
        let home = dirs::home_dir().expect("need home dir for tests");
        let fake_path = home.join("nonexistent_file_xyz_12345.png");
        let result = resolve_user_path(fake_path.to_str().unwrap(), &["png"]);
        assert_eq!(result, Err(SandboxError::NotFound));
    }

    #[test]
    fn dot_dot_escape_returns_outside_home_or_not_found() {
        let result = resolve_user_path("/tmp/../../../etc/passwd.png", &["png"]);
        assert!(
            result == Err(SandboxError::OutsideHome) || result == Err(SandboxError::NotFound),
            "Expected OutsideHome or NotFound, got {:?}",
            result
        );
    }

    #[test]
    fn path_outside_home_returns_outside_home() {
        let tmp_file = PathBuf::from("/tmp/.dither_yuki_sandbox_test.png");
        let _ = fs::File::create(&tmp_file).and_then(|mut f| f.write_all(b"test"));

        if tmp_file.exists() {
            let result = resolve_user_path(tmp_file.to_str().unwrap(), &["png"]);
            assert_eq!(result, Err(SandboxError::OutsideHome));
            let _ = fs::remove_file(&tmp_file);
        }
    }

    #[test]
    fn empty_allowed_ext_always_returns_bad_extension() {
        let home = dirs::home_dir().expect("need home dir for tests");
        let fake_path = home.join("test.png");
        let result = resolve_user_path(fake_path.to_str().unwrap(), &[]);
        assert_eq!(result, Err(SandboxError::BadExtension));
    }

    #[test]
    fn resolve_export_path_with_ensured_extension() {
        let home = dirs::home_dir().expect("need home dir for tests");
        let dir = home.join(".dither_yuki_test_sandbox").join("export_ensure");
        fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("newproj");
        let ensured = ensure_extension(raw.to_str().unwrap(), "dyproj");
        let resolved = resolve_export_path(&ensured, &["dyproj"]).unwrap();
        assert!(resolved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case("newproj.dyproj"));
        cleanup_subdir("export_ensure");
    }
}
