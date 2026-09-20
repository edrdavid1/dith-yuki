//! Clean-exit marker: absent on startup ⇒ previous run did not exit cleanly.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn marker_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("clean_exit.marker")
}

/// Call at startup before creating a new marker cycle.
/// Returns `true` when the previous process did not write a clean-exit marker
/// (crash / kill / first launch). Pair with a non-empty journal scan so first
/// launch does not show a recovery dialog.
pub fn previous_run_lacks_clean_marker(app_data_dir: &Path) -> bool {
    let path = marker_path(app_data_dir);
    let lacks = !path.exists();
    let _ = std::fs::remove_file(&path);
    lacks
}

/// Last action on clean exit (after allow_exit).
pub fn write_clean_exit_marker(app_data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_data_dir)?;
    let path = marker_path(app_data_dir);
    let mut f = File::create(&path)?;
    f.write_all(b"ok")?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(previous_run_lacks_clean_marker(dir.path()));
        write_clean_exit_marker(dir.path()).unwrap();
        assert!(!previous_run_lacks_clean_marker(dir.path()));
        assert!(!marker_path(dir.path()).exists());
    }
}
