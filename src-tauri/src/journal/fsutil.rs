//! Journal file/dir permissions (0600 / 0700) and durable writes.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

/// Ensure parent exists with mode 0700 (Unix) and write `bytes` atomically with 0600.
pub fn atomic_write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    ensure_dir_private(parent)?;
    // engine_io::atomic_write then chmod the final file.
    engine_io::atomic_write(path, bytes)?;
    set_file_private(path)?;
    Ok(())
}

pub fn ensure_dir_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if path.is_dir() {
            set_dir_private(path)?;
            return Ok(());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        set_dir_private(path)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

pub fn set_file_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        let _ = path; // ACL tightening deferred; MoveFileEx WRITE_THROUGH used upstream.
    }
    Ok(())
}

pub fn set_dir_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
    Ok(())
}

/// Write blob bytes via temp → fsync → rename into `blobs_dir` (0600 file).
pub fn write_blob_atomic(blobs_dir: &Path, filename: &str, bytes: &[u8]) -> std::io::Result<()> {
    ensure_dir_private(blobs_dir)?;
    let dest = blobs_dir.join(filename);
    if dest.is_file() {
        // Idempotent: already on disk for this (layer_id, raw_rev).
        return Ok(());
    }
    let tmp = blobs_dir.join(format!(".{filename}.tmp-{}", std::process::id()));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    set_file_private(&tmp)?;
    #[cfg(test)]
    {
        if let Err(e) = super::failpoint::check(super::failpoint::FailPoint::BeforeBlobRename) {
            // Leave tmp on disk (simulates kill before rename).
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
        }
    }
    fs::rename(&tmp, &dest)?;
    set_file_private(&dest)?;
    #[cfg(unix)]
    {
        if let Ok(dir) = File::open(blobs_dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(test)]
    {
        if let Err(e) = super::failpoint::check(super::failpoint::FailPoint::AfterBlobWrite) {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
        }
    }
    Ok(())
}
