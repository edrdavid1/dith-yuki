//! Session roster: last open-tab set for crash reopen prompts.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterEntry {
    pub recovery_id: Uuid,
    pub runtime_doc_id: u32,
    pub display_name: String,
    pub project_path: Option<String>,
    pub source_path: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionRoster {
    pub open_docs: Vec<RosterEntry>,
    pub active_recovery_id: Option<Uuid>,
}

pub fn roster_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("session_roster.json")
}

pub fn write_roster(app_data_dir: &Path, roster: &SessionRoster) -> Result<(), String> {
    fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(roster).map_err(|e| e.to_string())?;
    engine_io::atomic_write(&roster_path(app_data_dir), &json).map_err(|e| e.to_string())
}

pub fn read_roster(app_data_dir: &Path) -> Option<SessionRoster> {
    let bytes = fs::read(roster_path(app_data_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn clear_roster(app_data_dir: &Path) {
    let _ = fs::remove_file(roster_path(app_data_dir));
}

/// Snapshot current sessions into `session_roster.json` (call on tabs-changed).
pub fn persist_from_state(state: &AppState, app_data_dir: &Path) {
    let active_id = state.active_id();
    let Ok(map) = state.sessions.lock() else {
        return;
    };
    let mut open_docs = Vec::new();
    let mut active_recovery_id = None;
    for session in map.values() {
        let project_path = session
            .project_path
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|p| p.to_string_lossy().into_owned()));
        let source_path = session
            .source_path
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|p| p.to_string_lossy().into_owned()));
        let display_name = project_path
            .as_deref()
            .or(source_path.as_deref())
            .and_then(|p| Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Untitled {}", session.id.0));
        let live = session.document_handle.snapshot();
        let dirty = match session.history.saved_snapshot.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(saved) => !std::sync::Arc::ptr_eq(saved, &live),
                None => !live.root.is_empty(),
            },
            Err(_) => true,
        };
        if active_id == Some(session.id.0) {
            active_recovery_id = Some(session.recovery_id);
        }
        open_docs.push(RosterEntry {
            recovery_id: session.recovery_id,
            runtime_doc_id: session.id.0,
            display_name,
            project_path,
            source_path,
            dirty,
        });
    }
    open_docs.sort_by_key(|e| e.runtime_doc_id);
    let _ = write_roster(
        app_data_dir,
        &SessionRoster {
            open_docs,
            active_recovery_id,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let roster = SessionRoster {
            open_docs: vec![RosterEntry {
                recovery_id: Uuid::new_v4(),
                runtime_doc_id: 1,
                display_name: "a.dyproj".into(),
                project_path: Some("/tmp/a.dyproj".into()),
                source_path: None,
                dirty: true,
            }],
            active_recovery_id: None,
        };
        write_roster(dir.path(), &roster).unwrap();
        let loaded = read_roster(dir.path()).unwrap();
        assert_eq!(loaded.open_docs.len(), 1);
        assert_eq!(loaded.open_docs[0].display_name, "a.dyproj");
        clear_roster(dir.path());
        assert!(read_roster(dir.path()).is_none());
    }
}
