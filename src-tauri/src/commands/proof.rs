//! Soft-proof Tauri commands.
//!
//! Soft proof is view state: persisted in `.dyproj`, excluded from the undo stack
//! and from dirty/"Save changes?" (toggling preview must not prompt to save).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use engine_color::SoftProofConfig;
use tauri::State;

use crate::commands::{emit_document_changed, AppState};
use crate::services::proof_service::ProfileInfoDto;
use crate::undo::{emit_dirty_doc, is_dirty_doc, mark_clean_doc};

fn proof_service(state: &AppState) -> Result<Arc<crate::services::ProofService>, String> {
    state
        .proof
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "soft-proof catalog not initialized".into())
}

#[tauri::command]
pub fn proof_list_profiles(state: State<'_, Arc<AppState>>) -> Result<Vec<ProfileInfoDto>, String> {
    Ok(proof_service(state.inner())?.list_profiles())
}

#[tauri::command]
pub async fn proof_import_profile(
    path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ProfileInfoDto, String> {
    let svc = proof_service(state.inner())?;
    let path = PathBuf::from(path);
    // Import on a blocking pool with a soft timeout so a huge/hostile ICC cannot
    // freeze the UI thread.
    let import = tokio::task::spawn_blocking(move || svc.import_profile(path.as_path()));
    match tokio::time::timeout(std::time::Duration::from_secs(15), import).await {
        Ok(Ok(result)) => result.map_err(Into::into),
        Ok(Err(e)) => Err(format!("import task failed: {e}")),
        Err(_) => Err("ICC import timed out (15s)".into()),
    }
}

#[tauri::command]
pub fn proof_remove_profile(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    proof_service(state.inner())?
        .remove_profile(&id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn proof_get_config(
    doc_id: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<SoftProofConfig, String> {
    let session = state.require_session(doc_id)?;
    Ok(session.document_handle.snapshot().soft_proof.clone())
}

#[tauri::command]
pub fn proof_set_config(
    doc_id: u32,
    mut config: SoftProofConfig,
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SoftProofConfig, String> {
    let t0 = Instant::now();
    let svc = proof_service(state.inner())?;

    if let Some(name) = config.profile_display_name.take() {
        config.profile_display_name = Some(SoftProofConfig::sanitize_display_name(&name));
    }

    if config.enabled && !svc.has_profile(&config.profile_id) {
        let name = config
            .profile_display_name
            .clone()
            .unwrap_or_else(|| config.profile_id.clone());
        return Err(format!(
            "Profile {name} not found — import an ICC to continue"
        ));
    }
    if config.enabled {
        let build_t0 = Instant::now();
        let _ = svc.transform_for(&config).map_err(|e| e.to_string())?;
        log::info!(
            target: "soft_proof",
            "transform_build elapsed_ms={:.2}",
            build_t0.elapsed().as_secs_f64() * 1000.0
        );
        // Warm Perceptual/Absolute off the UI path after enable (Relative already built).
        svc.warm_alternate_intents(&config);
    }

    // Preserve display name from catalog when enabling.
    if config.profile_display_name.is_none() {
        if let Some(info) = svc
            .list_profiles()
            .into_iter()
            .find(|p| p.id == config.profile_id)
        {
            config.profile_display_name =
                Some(SoftProofConfig::sanitize_display_name(&info.name));
        }
    }

    let was_dirty = is_dirty_doc(state.inner(), doc_id);
    state
        .require_session(doc_id)?
        .document_handle
        .mutate(|doc| {
            doc.soft_proof = config.clone();
        });
    // Soft proof must not push undo or mark the document dirty.
    if !was_dirty {
        mark_clean_doc(state.inner(), doc_id);
    }
    emit_dirty_doc(Some(&app_handle), state.inner(), doc_id);
    emit_document_changed(&app_handle, "soft_proof_changed", None, Some(doc_id));
    log::info!(
        target: "soft_proof",
        "proof_set_config doc={} enabled={} elapsed_ms={:.2}",
        doc_id,
        config.enabled,
        t0.elapsed().as_secs_f64() * 1000.0
    );
    Ok(config)
}
