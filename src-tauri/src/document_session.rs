//! Per-process live document registry (`DocumentId` ≠ always 1).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use engine_project::document::DocumentHandle;
use engine_project::types::DocumentId;
use engine_project::Document;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::AppState;
use crate::undo::UndoManager;

/// One open document: tiles are keyed by `id`, undo/dirty/path are not shared.
pub struct DocumentSession {
    pub id: DocumentId,
    pub document_handle: DocumentHandle,
    pub undo_manager: Mutex<UndoManager>,
    pub saved_snapshot: Mutex<Option<Arc<Document>>>,
    pub project_path: Mutex<Option<PathBuf>>,
    /// In-flight save/export assemble count — close refuses while > 0.
    io_inflight: AtomicUsize,
}

/// RAII guard: keeps the session close-blocked for the duration of save/export.
pub struct SessionIoGuard {
    session: Arc<DocumentSession>,
}

impl Drop for SessionIoGuard {
    fn drop(&mut self) {
        self.session.io_inflight.fetch_sub(1, Ordering::AcqRel);
    }
}

impl DocumentSession {
    /// Pin this session against close for the duration of a blocking I/O assemble.
    pub fn begin_io(self: &Arc<Self>) -> SessionIoGuard {
        self.io_inflight.fetch_add(1, Ordering::AcqRel);
        SessionIoGuard {
            session: Arc::clone(self),
        }
    }

    pub fn io_inflight(&self) -> usize {
        self.io_inflight.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenDocumentTabDto {
    pub id: u32,
    pub title: String,
    pub dirty: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenDocumentsPayload {
    pub tabs: Vec<OpenDocumentTabDto>,
    pub active_id: Option<u32>,
}

impl AppState {
    pub fn empty_process(
        gpu: Option<std::sync::Arc<engine_gpu::GpuContext>>,
        cache_bytes: usize,
        dock_affinity_enabled: bool,
    ) -> Self {
        use std::sync::atomic::AtomicBool;

        let gpu_resident = gpu.as_ref().map(|ctx| {
            // Diag may request a tighter budget, but scratch (2×cap) must still fit and
            // leave room for a full origin viewport (~40 L0 tiles). Undersized budgets
            // used to yield 0 resident slots → silent frame failure → bogus ~0.01 ms p95.
            let cfg = if std::env::var("DITHER_GPU_RESIDENT_DIAG").is_ok() {
                engine_gpu::resident::VramBudgetConfig {
                    vram_budget_bytes: 256 * 1024 * 1024,
                    frame_batch_cap: 64,
                    ..engine_gpu::resident::default_vram_config()
                }
            } else {
                engine_gpu::resident::default_vram_config()
            };
            std::sync::Arc::new(engine_gpu::GpuTileCache::new(&ctx.device, cfg))
        });
        let gpu_executor = gpu_resident.as_ref().and_then(|cache| {
            gpu.as_ref().and_then(|ctx| {
                engine_gpu::GpuExecutor::spawn(std::sync::Arc::clone(ctx), std::sync::Arc::clone(cache))
                    .ok()
                    .map(|ex| std::sync::Mutex::new(ex))
            })
        });

        Self {
            sessions: Mutex::new(HashMap::new()),
            next_doc_id: AtomicU32::new(1),
            active_id: Mutex::new(None),
            tile_cache: engine_tiles::TileCache::new(cache_bytes),
            scheduler: engine_tiles::Scheduler::new(),
            viewport: Mutex::new(crate::viewport::ViewportState::default()),
            worker_wake: crate::worker::WorkerWake::new(),
            palette_cache: engine_color::palette_cache::PaletteKdCache::new(),
            palette_lut_cache: engine_color::palette_lut::PaletteLutCache::new(),
            threshold_cache: engine_color::threshold_map::ThresholdMapCache::new(),
            error_residuals: engine_project::filters::ErrorResidualsStore::new(),
            block_representatives: engine_tiles::BlockRepresentativeCache::new(),
            ed_frontier: engine_tiles::EdFrontier::new(),
            gpu,
            gpu_resident,
            gpu_executor,
            app_handle: Mutex::new(None),
            panel_manager: Mutex::new(crate::panel_manager::PanelManager::new()),
            selection: Mutex::new(crate::commands::SelectionState::default()),
            dock_affinity: Mutex::new(crate::dock_affinity::DockAffinityController::new(
                dock_affinity_enabled,
            )),
            float_drag_mouseup_cancel: Arc::new(AtomicBool::new(true)),
            float_drag_mouseup_hook: Mutex::new(None),
            preview_pass_inflight: AtomicUsize::new(0),
            pending_preview_refresh: Mutex::new(None),
        }
    }

    pub fn alloc_doc_id(&self) -> u32 {
        self.next_doc_id.fetch_add(1, Ordering::Relaxed)
    }

    fn bump_next_past(&self, id: u32) {
        let mut cur = self.next_doc_id.load(Ordering::Relaxed);
        while id >= cur {
            match self.next_doc_id.compare_exchange(
                cur,
                id + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    pub fn spawn_session(&self, doc: Document) -> Arc<DocumentSession> {
        let id = doc.id;
        self.bump_next_past(id.0);
        let session = Arc::new(DocumentSession {
            id,
            document_handle: DocumentHandle::new(doc),
            undo_manager: Mutex::new(UndoManager::new()),
            saved_snapshot: Mutex::new(None),
            project_path: Mutex::new(None),
            io_inflight: AtomicUsize::new(0),
        });
        if let Ok(mut map) = self.sessions.lock() {
            map.insert(id.0, session.clone());
        }
        if let Ok(mut active) = self.active_id.lock() {
            *active = Some(id.0);
        }
        session
    }

    pub fn session(&self, doc: u32) -> Result<Arc<DocumentSession>, String> {
        let map = self.sessions.lock().map_err(|e| e.to_string())?;
        map.get(&doc)
            .cloned()
            .ok_or_else(|| format!("No document session {doc}"))
    }

    /// Resolve a session by explicit runtime id (Photoshop/Figma/VS Code style).
    /// Prefer this over [`Self::active_session`] for any mutating IPC.
    pub fn require_session(&self, doc_id: u32) -> Result<Arc<DocumentSession>, String> {
        self.session(doc_id).map_err(|_| {
            format!("Document was closed; cannot apply change (id {doc_id})")
        })
    }

    pub fn active_id(&self) -> Option<u32> {
        self.active_id.lock().ok().and_then(|g| *g)
    }

    /// Active-tab helper for viewport / chrome reads. **Do not use for mutations** —
    /// pass explicit `doc_id` and [`Self::require_session`] instead (cross-tab race).
    pub fn active_session(&self) -> Result<Arc<DocumentSession>, String> {
        let id = self
            .active_id()
            .ok_or_else(|| "No document open".to_string())?;
        self.session(id)
    }

    pub fn must_active(&self) -> Arc<DocumentSession> {
        self.active_session()
            .expect("AppState has no active document")
    }

    /// Tab strip order (monotonic id ascending — matches current UI sort).
    pub fn tab_ids_in_order(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.open_doc_ids().into_iter().collect();
        ids.sort_unstable();
        ids
    }

    /// Chrome/VS Code neighbor: prefer tab to the right of `closed`, else left.
    pub fn neighbor_after_close(order: &[u32], closed: u32) -> Option<u32> {
        let Some(idx) = order.iter().position(|&id| id == closed) else {
            return order.last().copied();
        };
        if idx + 1 < order.len() {
            Some(order[idx + 1])
        } else if idx > 0 {
            Some(order[idx - 1])
        } else {
            None
        }
    }

    /// Runtime document ids currently in the session map (for Raw pin set).
    pub fn open_doc_ids(&self) -> HashSet<u32> {
        self.sessions
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn activate(&self, doc: u32) -> Result<Arc<DocumentSession>, String> {
        let session = self.session(doc)?;
        if let Ok(mut active) = self.active_id.lock() {
            *active = Some(doc);
        }
        // Do NOT soft-trim Processed/Composite of the previous tab: that blanks
        // return-to-tab preview. Under pressure, drop inactive non-Raw only.
        self.evict_inactive_for_pressure_if_needed();
        Ok(session)
    }

    /// Pressure with empty viewport protect set (inactive-only when active is set).
    /// Open-session Raw is always pinned via `open_docs`.
    pub fn evict_inactive_for_pressure_if_needed(&self) {
        if self.tile_cache.used_bytes_count() <= self.tile_cache.budget_bytes_count() {
            return;
        }
        let empty = HashSet::new();
        let open_docs = self.open_doc_ids();
        self.tile_cache
            .evict_for_pressure(&engine_tiles::EvictContext {
                active_doc: self.active_id(),
                open_docs: &open_docs,
                viewport_coords: &empty,
            });
    }

    /// Build `EvictContext` from active tab + viewport + open sessions.
    pub fn evict_for_pressure_if_needed(&self) {
        if self.tile_cache.used_bytes_count() <= self.tile_cache.budget_bytes_count() {
            return;
        }
        let viewport_coords: HashSet<engine_tiles::TileCoord> = self
            .viewport
            .lock()
            .map(|v| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default();
        let open_docs = self.open_doc_ids();
        self.tile_cache
            .evict_for_pressure(&engine_tiles::EvictContext {
                active_doc: self.active_id(),
                open_docs: &open_docs,
                viewport_coords: &viewport_coords,
            });
    }

    pub fn close_session(&self, doc: u32) -> Result<(), String> {
        // Capture strip order before remove (Chrome/VS Code neighbor activation).
        let order_before = self.tab_ids_in_order();
        {
            let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
            let session = map
                .get(&doc)
                .ok_or_else(|| format!("No document session {doc}"))?;
            if session.io_inflight() > 0 {
                return Err(
                    "Cannot close document while save or export is in progress".to_string(),
                );
            }
            map.remove(&doc)
                .ok_or_else(|| format!("No document session {doc}"))?;
        }
        self.tile_cache.evict_document(doc);
        if let Some(gpu_cache) = &self.gpu_resident {
            gpu_cache.evict_document(doc);
        }
        self.error_residuals.evict_document(doc);
        self.block_representatives.evict_document(doc);
        self.ed_frontier.evict_document(doc);
        self.palette_cache.evict_document(doc);
        self.palette_lut_cache.evict_document(doc);

        let mut active = self.active_id.lock().map_err(|e| e.to_string())?;
        if *active == Some(doc) {
            *active = Self::neighbor_after_close(&order_before, doc);
        }
        Ok(())
    }

    pub fn tab_list(&self) -> OpenDocumentsPayload {
        let active_id = self.active_id();
        let map = match self.sessions.lock() {
            Ok(g) => g,
            Err(_) => {
                return OpenDocumentsPayload {
                    tabs: vec![],
                    active_id: None,
                };
            }
        };
        let mut tabs: Vec<OpenDocumentTabDto> = map
            .values()
            .map(|s| {
                let path = s
                    .project_path
                    .lock()
                    .ok()
                    .and_then(|p| p.as_ref().map(|p| p.to_string_lossy().into_owned()));
                let title = path
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_name())
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Untitled {}", s.id.0));
                let live = s.document_handle.snapshot();
                let dirty = match s.saved_snapshot.lock() {
                    Ok(guard) => match guard.as_ref() {
                        Some(saved) => !Arc::ptr_eq(saved, &live),
                        None => !live.root.is_empty(),
                    },
                    Err(_) => true,
                };
                OpenDocumentTabDto {
                    id: s.id.0,
                    title,
                    dirty,
                    path,
                }
            })
            .collect();
        tabs.sort_by_key(|t| t.id);
        OpenDocumentsPayload { tabs, active_id }
    }
}

pub fn emit_tabs_changed(app: Option<&AppHandle>, state: &AppState) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit("tabs-changed", state.tab_list());
}

#[cfg(test)]
mod pressure_tests {
    use super::*;
    use engine_project::types::DocumentId;
    use engine_project::Document;
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, TILE_BYTES};
    use std::sync::Arc;

    fn fill_stage(cache: &engine_tiles::TileCache, doc: u32, stage: CacheStage, n: usize) {
        for i in 0..n {
            let key = TileKey {
                doc,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: i as u32,
                    y: 0,
                },
                stage,
            };
            cache.get_or_insert(key, Arc::new(PixelTile::new()));
        }
    }

    #[test]
    fn two_session_pressure_pins_open_raw_drops_inactive_composite() {
        let state = AppState::empty_process(None, TILE_BYTES, true);
        state.spawn_session(Document::new(DocumentId::new(1), 64, 64));
        state.spawn_session(Document::new(DocumentId::new(2), 64, 64));
        fill_stage(&state.tile_cache, 1, CacheStage::Raw, 1);
        fill_stage(&state.tile_cache, 1, CacheStage::Composite, 1);
        fill_stage(&state.tile_cache, 2, CacheStage::Raw, 1);
        assert!(state.tile_cache.used_bytes_count() > state.tile_cache.budget_bytes_count());

        {
            let mut vp = state.viewport.lock().unwrap();
            vp.visible_tiles = vec![TileCoord {
                level: 0,
                x: 0,
                y: 0,
            }];
        }
        state.activate(2).unwrap();
        state.evict_for_pressure_if_needed();

        assert!(
            !state.tile_cache.entries.contains_key(&TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0
                },
                stage: CacheStage::Composite,
            }),
            "inactive Composite should be dropped"
        );
        assert!(
            state.tile_cache.entries.contains_key(&TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0
                },
                stage: CacheStage::Raw,
            }),
            "open-session Raw must stay pinned"
        );
        assert!(state.tile_cache.entries.contains_key(&TileKey {
            doc: 2,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0
            },
            stage: CacheStage::Raw,
        }));
    }

    #[test]
    fn activate_preserves_background_composite_when_under_budget() {
        let state = AppState::empty_process(None, 10_000_000, true);
        state.spawn_session(Document::new(DocumentId::new(1), 64, 64));
        state.spawn_session(Document::new(DocumentId::new(2), 64, 64));
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        for stage in [
            CacheStage::Raw,
            CacheStage::Processed,
            CacheStage::Composite,
        ] {
            state.tile_cache.get_or_insert(
                TileKey {
                    doc: 1,
                    layer: 1,
                    coord,
                    stage,
                },
                Arc::new(PixelTile::new()),
            );
        }
        state.activate(1).unwrap();
        state.activate(2).unwrap();

        assert!(state.tile_cache.entries.contains_key(&TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Composite,
        }));
    }

    #[test]
    fn single_doc_pressure_pins_open_raw_drops_off_viewport_composite() {
        let state = AppState::empty_process(None, TILE_BYTES, true);
        state.spawn_session(Document::new(DocumentId::new(1), 1024, 1024));
        fill_stage(&state.tile_cache, 1, CacheStage::Raw, 1);
        fill_stage(&state.tile_cache, 1, CacheStage::Composite, 3);
        {
            let mut vp = state.viewport.lock().unwrap();
            vp.visible_tiles = vec![TileCoord {
                level: 0,
                x: 0,
                y: 0,
            }];
        }
        state.evict_for_pressure_if_needed();
        assert!(state.tile_cache.entries.contains_key(&TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0
            },
            stage: CacheStage::Raw,
        }));
        assert!(state.tile_cache.entries.contains_key(&TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0
            },
            stage: CacheStage::Composite,
        }));
        assert!(!state.tile_cache.entries.contains_key(&TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0
            },
            stage: CacheStage::Composite,
        }));
    }

    #[test]
    fn close_rejected_while_io_inflight() {
        let state = AppState::empty_process(None, 10_000_000, true);
        let session = state.spawn_session(Document::new(DocumentId::new(1), 64, 64));
        let _guard = session.begin_io();
        let err = state.close_session(1).unwrap_err();
        assert!(err.contains("save or export"), "{err}");
        drop(_guard);
        assert!(state.close_session(1).is_ok());
    }

    #[test]
    fn neighbor_after_close_prefers_right_then_left() {
        assert_eq!(
            AppState::neighbor_after_close(&[1, 2, 3], 2),
            Some(3),
            "middle → right"
        );
        assert_eq!(
            AppState::neighbor_after_close(&[1, 2, 3], 3),
            Some(2),
            "last → left"
        );
        assert_eq!(
            AppState::neighbor_after_close(&[1, 2, 3], 1),
            Some(2),
            "first → right"
        );
        assert_eq!(AppState::neighbor_after_close(&[7], 7), None);
    }

    #[test]
    fn close_active_activates_neighbor_not_max_id() {
        let state = AppState::empty_process(None, 10_000_000, true);
        state.spawn_session(Document::new(DocumentId::new(1), 64, 64));
        state.spawn_session(Document::new(DocumentId::new(2), 64, 64));
        state.spawn_session(Document::new(DocumentId::new(3), 64, 64));
        state.activate(2).unwrap();
        state.close_session(2).unwrap();
        assert_eq!(
            state.active_id(),
            Some(3),
            "closing middle active should select right neighbor, not max-only coincidence"
        );
        state.activate(3).unwrap();
        state.close_session(3).unwrap();
        assert_eq!(state.active_id(), Some(1));
    }
}
