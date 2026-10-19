//! Startup tmp cleanup and 512 MiB blob GC (H9 §4.4 / Step 5).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Remove incomplete blob temps and restore orphaned `.claiming` paths after a crash.
pub fn startup_cleanup(recovery_dir: &Path) {
    let Ok(entries) = fs::read_dir(recovery_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Restore claim interrupted mid-recover: prefer durable names again.
        if let Some(stem) = name.strip_suffix(".state.claiming") {
            let dest = recovery_dir.join(format!("{stem}.state"));
            if !dest.exists() {
                let _ = fs::rename(&path, &dest);
            } else {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        if let Some(stem) = name.strip_suffix(".blobs.claiming") {
            let dest = recovery_dir.join(format!("{stem}.blobs"));
            if !dest.exists() {
                let _ = fs::rename(&path, &dest);
            } else {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }
        if name.ends_with(".dyproj.journal.claiming") {
            let dest_name = name.trim_end_matches(".claiming");
            let dest = recovery_dir.join(dest_name);
            if !dest.exists() {
                let _ = fs::rename(&path, &dest);
            } else {
                let _ = fs::remove_file(&path);
            }
            continue;
        }

        // Incomplete atomic blob temps: `.{filename}.tmp-{pid}`
        if name.starts_with('.') && name.contains(".tmp-") {
            let _ = fs::remove_file(&path);
            continue;
        }

        if name.ends_with(".blobs") && path.is_dir() {
            remove_tmp_in_dir(&path);
        }
    }
}

fn remove_tmp_in_dir(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') && name.contains(".tmp-") {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Delete blob files not in `keep` (filenames only). Returns approximate bytes freed.
pub fn gc_unreferenced_blobs(blobs_dir: &Path, keep: &HashSet<String>) -> u64 {
    let Ok(entries) = fs::read_dir(blobs_dir) else {
        return 0;
    };
    let mut freed = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            // Drop leftover temps under pressure too.
            if let Ok(m) = entry.metadata() {
                freed = freed.saturating_add(m.len());
            }
            let _ = fs::remove_file(&path);
            continue;
        }
        if keep.contains(name) {
            continue;
        }
        if let Ok(m) = entry.metadata() {
            freed = freed.saturating_add(m.len());
        }
        let _ = fs::remove_file(&path);
    }
    freed
}

/// Paths used while a StateV1 journal is claimed for recover.
pub fn state_claiming_path(recovery_dir: &Path, recovery_id: uuid::Uuid) -> PathBuf {
    recovery_dir.join(format!("{recovery_id}.state.claiming"))
}

pub fn blobs_claiming_path(recovery_dir: &Path, recovery_id: uuid::Uuid) -> PathBuf {
    recovery_dir.join(format!("{recovery_id}.blobs.claiming"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn startup_removes_tmp_and_restores_claim() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&recovery).unwrap();
        let id = uuid::Uuid::new_v4();
        let claiming = recovery.join(format!("{id}.state.claiming"));
        fs::write(&claiming, b"state").unwrap();
        let blobs = recovery.join(format!("{id}.blobs"));
        fs::create_dir_all(&blobs).unwrap();
        let mut tmp = fs::File::create(blobs.join(".layer-1-1.png.tmp-99999")).unwrap();
        tmp.write_all(b"partial").unwrap();

        startup_cleanup(&recovery);

        assert!(recovery.join(format!("{id}.state")).is_file());
        assert!(!claiming.exists());
        assert!(!blobs.join(".layer-1-1.png.tmp-99999").exists());
    }

    #[test]
    fn gc_keeps_referenced_only() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs");
        fs::create_dir_all(&blobs).unwrap();
        fs::write(blobs.join("layer-1-1.png"), b"keep").unwrap();
        fs::write(blobs.join("layer-1-0.png"), b"drop").unwrap();
        let mut keep = HashSet::new();
        keep.insert("layer-1-1.png".into());
        let freed = gc_unreferenced_blobs(&blobs, &keep);
        assert!(freed >= 4);
        assert!(blobs.join("layer-1-1.png").is_file());
        assert!(!blobs.join("layer-1-0.png").exists());
    }
}
