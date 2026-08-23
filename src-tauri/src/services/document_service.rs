use std::sync::Arc;
use tauri::AppHandle;

use serde::{Deserialize, Serialize};
use engine_tiles::PixelTile;
use engine_project::types::LayerKind;
use engine_project::commands as engine_commands;
use engine_project::commands::AddLayerArgs;

use crate::commands::{emit_document_changed, schedule_dirty_viewport_tiles, AppState};
use crate::document_session::emit_tabs_changed;
use crate::services::{AppError, layer_service::LayerIdResponse};

pub const MAX_DOCUMENT_DIMENSION: u32 = 8192;
pub const IMAGE_IMPORT_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlankBackground {
    Transparent,
    White,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    pub snapshot: engine_project::dto::DocumentSnapshotDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadImageResponse {
    pub doc_id: u32,
    pub width: u32,
    pub height: u32,
    pub tile_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveProjectResponse {
    pub path: String,
    pub size_warning: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenProjectResponse {
    pub doc_id: u32,
    pub width: u32,
    pub height: u32,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportPatternRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_instance_ids: Option<Vec<String>>,
    pub path: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportPatternRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub path: String,
    pub target_layer_id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPatternResponse {
    pub filter_ids: Vec<String>,
    pub palette_ids: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportImageRequest {
    pub doc_id: u32,
    pub path: String,
    pub format: String,
    pub quality: Option<u8>,
    #[serde(default)]
    pub svg_algorithm: Option<String>,
}

pub fn validate_document_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Invalid state: image has zero dimensions".to_string());
    }
    if width > MAX_DOCUMENT_DIMENSION || height > MAX_DOCUMENT_DIMENSION {
        return Err(format!(
            "Invalid state: image dimensions {}x{} exceed maximum {}x{}",
            width, height, MAX_DOCUMENT_DIMENSION, MAX_DOCUMENT_DIMENSION
        ));
    }
    Ok(())
}

fn decode_image_to_rgba_f32(path: &str) -> Result<(u32, u32, Vec<f32>), String> {
    let img = image::open(path).map_err(|e| format!("IO error: {e}"))?;
    let img_rgba = img.to_rgba8();
    let width = img_rgba.width();
    let height = img_rgba.height();
    validate_document_dimensions(width, height)?;

    let pixel_count = (width as usize) * (height as usize);
    let mut rgba_f32 = Vec::with_capacity(pixel_count * 4);
    for pixel in img_rgba.pixels() {
        rgba_f32.push(pixel[0] as f32 / 255.0);
        rgba_f32.push(pixel[1] as f32 / 255.0);
        rgba_f32.push(pixel[2] as f32 / 255.0);
        rgba_f32.push(pixel[3] as f32 / 255.0);
    }
    Ok((width, height, rgba_f32))
}

pub fn place_image_at_origin(
    src: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<f32> {
    let mut dst = vec![0.0; (dst_w as usize) * (dst_h as usize) * 4];
    let copy_w = src_w.min(dst_w) as usize;
    let copy_h = src_h.min(dst_h) as usize;
    let src_stride = src_w as usize * 4;
    let dst_stride = dst_w as usize * 4;
    let row_bytes = copy_w * 4;
    for y in 0..copy_h {
        let src_row = y * src_stride;
        let dst_row = y * dst_stride;
        dst[dst_row..dst_row + row_bytes].copy_from_slice(&src[src_row..src_row + row_bytes]);
    }
    dst
}

pub fn blank_rgba_f32(width: u32, height: u32, background: BlankBackground) -> Vec<f32> {
    let n = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    match background {
        BlankBackground::Transparent => vec![0.0; n],
        BlankBackground::White => vec![1.0; n],
    }
}

pub fn install_raster_document(
    state: &AppState,
    width: u32,
    height: u32,
    rgba_f32: &[f32],
    app: Option<&AppHandle>,
) -> Result<LoadImageResponse, String> {
    use engine_project::types::DocumentId;
    use engine_tiles::decompose::decompose_image_to_tiles_at_generation;

    let doc_id = state.alloc_doc_id();
    let live_gen = 1u64;
    let layer_id = 1u32;
    let grid = decompose_image_to_tiles_at_generation(
        rgba_f32, width, height, doc_id, layer_id, &state.tiles.tile_cache, live_gen,
    )
    .map_err(|e| format!("Tile decomposition error: {}", e))?;

    let mut new_doc = engine_project::Document::new(DocumentId::new(doc_id), width, height);
    let layer = engine_project::layer::Layer::new(
        engine_project::types::LayerId::new(1),
        engine_project::types::LayerKind::Raster,
        width,
        height,
    );
    new_doc.root.push(engine_project::layer::LayerNode::Leaf(layer));
    new_doc.increment_generation();
    new_doc.generations.set_document_gen(live_gen);

    let session = state.spawn_session(new_doc);
    state.evict_inactive_for_pressure_if_needed();
    crate::undo::clear_history(state, app, doc_id)?;
    emit_tabs_changed(app, state);

    schedule_dirty_viewport_tiles(state);

    let _ = session;

    Ok(LoadImageResponse {
        doc_id,
        width,
        height,
        tile_count: grid.cols * grid.rows,
    })
}

pub fn import_raster_layer(
    state: &AppState,
    doc_id: u32,
    src_w: u32,
    src_h: u32,
    src_rgba: &[f32],
    app: Option<&AppHandle>,
) -> Result<LayerIdResponse, String> {
    use engine_tiles::decompose::decompose_image_to_tiles_at_generation;

    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    if snapshot.root.is_empty() {
        return Err("No document open".to_string());
    }
    let dst_w = snapshot.width;
    let dst_h = snapshot.height;
    let engine_doc_id = snapshot.id;
    let insert_index = snapshot.root.len();
    drop(snapshot);

    let placed = place_image_at_origin(src_rgba, src_w, src_h, dst_w, dst_h);

    crate::undo::with_document_undo(state, app, doc_id, || {
        let args = AddLayerArgs {
            kind: LayerKind::Raster,
            parent_group: None,
            index: insert_index,
            width: dst_w,
            height: dst_h,
        };
        let layer_id = engine_commands::add_layer(
            &state.require_session(doc_id)?.document_handle,
            &state.tiles.tile_cache,
            engine_doc_id,
            args,
        )
        .map_err(|e| format!("Failed to add layer: {e:?}"))?;

        let live_gen = state
            .require_session(doc_id)?
            .document_handle
            .snapshot()
            .generations
            .current_document_gen();
        decompose_image_to_tiles_at_generation(
            &placed, dst_w, dst_h, doc_id, layer_id.0, &state.tiles.tile_cache, live_gen,
        )
            .map_err(|e| format!("Tile decomposition error: {e}"))?;

        state.evict_inactive_for_pressure_if_needed();

        if let Some(handle) = app {
            emit_document_changed(handle, "layer_added", Some(layer_id.0), Some(doc_id));
        }
        if state.active_id() == Some(doc_id) {
            schedule_dirty_viewport_tiles(state);
        }
        Ok(LayerIdResponse {
            layer_id: layer_id.0,
        })
    })
}

pub fn f32_to_u8(val: f32) -> u8 {
    (val * 255.0).clamp(0.0, 255.0) as u8
}

pub fn encode_rgba_to_png(buffer: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    use std::io::Cursor;

    let mut png_data: Vec<u8> = Vec::new();
    let cursor = Cursor::new(&mut png_data);
    let encoder = PngEncoder::new(cursor);
    encoder
        .write_image(buffer, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG encoding error: {}", e))?;

    Ok(png_data)
}

fn find_first_visible_layer(nodes: &[engine_project::LayerNode]) -> Option<&engine_project::Layer> {
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) => {
                if layer.visible {
                    return Some(layer);
                }
            }
            engine_project::LayerNode::Group(group) => {
                if group.visible {
                    if let Some(layer) = find_first_visible_layer(&group.children) {
                        return Some(layer);
                    }
                }
            }
        }
    }
    None
}

pub struct DocumentService {
    state: Arc<AppState>,
}

impl DocumentService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn new_document(
        &self,
        width: u32,
        height: u32,
        app_handle: &AppHandle,
    ) -> Result<DocumentResponse, AppError> {
        use engine_project::types::DocumentId;
        
        let new_doc = engine_project::Document::new(DocumentId::new(self.state.alloc_doc_id()), width, height);
        let session = self.state.spawn_session(new_doc);
        let doc_id = session.id.0;
        crate::undo::clear_history(&self.state, Some(app_handle), doc_id)?;
        emit_tabs_changed(Some(app_handle), &self.state);
        
        let snapshot = session.document_handle.snapshot();
        let dto = engine_project::dto::document_to_dto(&snapshot);
        
        Ok(DocumentResponse { snapshot: dto })
    }

    pub fn get_document_snapshot(&self) -> Result<DocumentResponse, AppError> {
        let Ok(session) = self.state.active_session() else {
            let empty = engine_project::Document::new(engine_project::types::DocumentId::new(0), 0, 0);
            let dto = engine_project::dto::document_to_dto(&empty);
            return Ok(DocumentResponse { snapshot: dto });
        };
        let snapshot = session.document_handle.snapshot();
        let dto = engine_project::dto::document_to_dto(&snapshot);
        Ok(DocumentResponse { snapshot: dto })
    }

    pub fn load_image_blocking(
        &self,
        path: &str,
        app_handle: &AppHandle,
    ) -> Result<LoadImageResponse, AppError> {
        let (width, height, rgba_f32) = decode_image_to_rgba_f32(path)?;
        let response = install_raster_document(&self.state, width, height, &rgba_f32, Some(app_handle))?;
        emit_document_changed(app_handle, "image_loaded", None, Some(response.doc_id));
        crate::recent_files::record_from_app(
            app_handle,
            path,
            crate::recent_files::RecentFileKind::Image,
        );
        Ok(response)
    }

    pub fn create_document_blocking(
        &self,
        width: u32,
        height: u32,
        background: BlankBackground,
        app_handle: &AppHandle,
    ) -> Result<LoadImageResponse, AppError> {
        validate_document_dimensions(width, height)?;
        let rgba_f32 = blank_rgba_f32(width, height, background);
        let response = install_raster_document(&self.state, width, height, &rgba_f32, Some(app_handle))?;
        emit_document_changed(app_handle, "document_created", None, Some(response.doc_id));
        Ok(response)
    }

    pub fn import_image_layer_blocking(
        &self,
        doc_id: u32,
        path: &str,
        app_handle: &AppHandle,
    ) -> Result<LayerIdResponse, AppError> {
        use engine_io::sandbox;

        let resolved = sandbox::resolve_user_path(path, IMAGE_IMPORT_EXTENSIONS)
            .map_err(|e| format!("Path error: {e}"))?;
        let resolved_str = resolved.to_string_lossy().into_owned();

        let (width, height, rgba_f32) = decode_image_to_rgba_f32(&resolved_str)?;
        import_raster_layer(&self.state, doc_id, width, height, &rgba_f32, Some(app_handle))
            .map_err(|e| AppError::Generic(e))
    }

    pub async fn save_project(
        &self,
        doc_id: u32,
        path: Option<String>,
        app_handle: AppHandle,
    ) -> Result<SaveProjectResponse, AppError> {
        let target = match path {
            Some(p) => p,
            None => {
                let session = self.state.require_session(doc_id)?;
                let guard = session
                    .project_path
                    .lock()
                    .map_err(|e| format!("Lock error: {e}"))?;
                guard
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .ok_or_else(|| AppError::Generic("Save As required: no project path set".to_string()))?
            }
        };
        self.save_project_as(doc_id, target, app_handle).await
    }

    pub async fn save_project_as(
        &self,
        doc_id: u32,
        path: String,
        app_handle: AppHandle,
    ) -> Result<SaveProjectResponse, AppError> {
        use engine_io::sandbox;
        use engine_project::serialize::{read_png_file, save_project_to_path};
        use engine_project::serialize::ProjectError;

        let resolved = sandbox::resolve_export_path(&path, &["dyproj"])
            .map_err(|e| format!("Path error: {e}"))?;

        let session = self.state.require_session(doc_id)?;
        let _io_guard = session.begin_io();
        let snapshot = session.document_handle.snapshot();
        let doc = (*snapshot).clone();
        drop(snapshot);

        let state_arc = Arc::clone(&self.state);
        let resolved_clone = resolved.clone();

        let result = tauri::async_runtime::spawn_blocking(move || {
            save_project_to_path(
                &resolved_clone,
                &doc,
                &state_arc.tiles.tile_cache,
                env!("CARGO_PKG_VERSION"),
                |p| read_png_file(p),
            )
        })
        .await
        .map_err(|e| format!("Save error: {e}"))?
        .map_err(|e| match e {
            ProjectError::IncompleteRaw { doc_id, layer_id } => format!(
                "Cannot save: image tiles missing from memory for document {doc_id} layer {layer_id} — reopen the file"
            ),
            other => format!("Save error: {other}"),
        })?;

        if let Ok(mut guard) = session.project_path.lock() {
            *guard = Some(resolved.clone());
        }

        let stored = resolved.to_string_lossy().into_owned();
        crate::recent_files::record_from_app(
            &app_handle,
            &stored,
            crate::recent_files::RecentFileKind::Project,
        );

        crate::undo::mark_clean(&self.state);
        crate::undo::emit_dirty(Some(&app_handle), &self.state);

        Ok(SaveProjectResponse {
            path: stored,
            size_warning: result.size_warning,
        })
    }

    pub async fn open_project(
        &self,
        path: String,
        app_handle: AppHandle,
    ) -> Result<OpenProjectResponse, AppError> {
        use engine_io::sandbox;
        use engine_project::serialize::open_project_from_bytes;
        use engine_project::types::DocumentId;
        use engine_tiles::TileCache;
        use std::fs;

        let resolved = sandbox::resolve_user_path(&path, &["dyproj"])
            .map_err(|e| format!("Path error: {e}"))?;

        let zip_bytes = tauri::async_runtime::spawn_blocking({
            let resolved = resolved.clone();
            move || fs::read(&resolved).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("Open error: {e}"))??;

        let runtime_id = self.state.alloc_doc_id();
        let staging = TileCache::new(self.state.tiles.tile_cache.budget_bytes_count());
        let opened = tauri::async_runtime::spawn_blocking(move || {
            open_project_from_bytes(&zip_bytes, &staging, DocumentId::new(runtime_id))
                .map(|r| (r, staging))
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("Open error: {e}"))??;

        let (opened, staging) = opened;
        let live_gen = 1u64;

        for entry in staging.entries.iter() {
            let key = *entry.key();
            let tile = entry.value().tile.clone();
            let _ = self.state.tiles.tile_cache.insert_fresh_gen(key, tile, live_gen);
        }

        let width = opened.document.width;
        let height = opened.document.height;
        let mut new_doc = opened.document;
        new_doc.increment_generation();
        new_doc.generations.set_document_gen(live_gen);
        let session = self.state.spawn_session(new_doc);
        self.state.evict_inactive_for_pressure_if_needed();
        let doc_id = runtime_id;
        crate::undo::clear_history(&self.state, Some(&app_handle), doc_id)?;
        crate::undo::mark_clean_doc(&self.state, doc_id);

        schedule_dirty_viewport_tiles(&self.state);

        if let Ok(mut guard) = session.project_path.lock() {
            *guard = Some(resolved.clone());
        }

        emit_document_changed(&app_handle, "project_opened", None, Some(doc_id));
        emit_tabs_changed(Some(&app_handle), &self.state);

        let stored = resolved.to_string_lossy().into_owned();
        crate::recent_files::record_from_app(
            &app_handle,
            &stored,
            crate::recent_files::RecentFileKind::Project,
        );

        Ok(OpenProjectResponse {
            doc_id: runtime_id,
            width,
            height,
            path: stored,
        })
    }

    pub fn export_pattern(
        &self,
        req: ExportPatternRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        use engine_io::sandbox;
        use engine_project::serialize::{
            export_pattern_from_document, read_png_file, write_pattern_to_path, PatternExportMeta,
        };
        use engine_project::types::FilterInstanceId;
        use uuid::Uuid;

        let resolved = sandbox::resolve_export_path(&req.path, &["dyuki"])
            .map_err(|e| format!("Path error: {e}"))?;

        let ids: Option<Vec<FilterInstanceId>> = match &req.filter_instance_ids {
            None => None,
            Some(list) if list.is_empty() => None,
            Some(list) => {
                let parsed = list
                    .iter()
                    .map(|s| {
                        Uuid::parse_str(s)
                            .map(FilterInstanceId)
                            .map_err(|e| format!("Invalid filter id '{s}': {e}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Some(parsed)
            }
        };

        let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
        let zip = export_pattern_from_document(
            &snapshot,
            engine_project::types::LayerId::new(req.layer_id),
            ids.as_deref(),
            &PatternExportMeta {
                name: req.name.unwrap_or_default(),
                description: req.description,
                author: None,
            },
            env!("CARGO_PKG_VERSION"),
            |p| read_png_file(p),
        )
        .map_err(|e| e.to_string())?;

        write_pattern_to_path(&resolved, &zip).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn import_pattern(
        &self,
        req: ImportPatternRequest,
        app_handle: &AppHandle,
    ) -> Result<ImportPatternResponse, AppError> {
        let doc_id = req.doc_id;
        use engine_io::sandbox;
        use engine_project::filter::FilterParams;
        use engine_project::serialize::import_pattern_into_document;
        use std::fs;

        let resolved = sandbox::resolve_user_path(&req.path, &["dyuki"])
            .map_err(|e| format!("Path error: {e}"))?;

        let zip_bytes = fs::read(&resolved).map_err(|e| format!("Read error: {e}"))?;

        let state = Arc::clone(&self.state);
        let target_layer_id = req.target_layer_id;
        let app_handle = app_handle.clone();
        let result = crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
            let mut imported_filters_are_dither = false;
            let mut err: Option<String> = None;
            let mut out: Option<engine_project::serialize::ImportPatternResult> = None;
            state.require_session(doc_id)?.document_handle.mutate(|doc| {
            match import_pattern_into_document(
                &zip_bytes,
                doc,
                engine_project::types::LayerId::new(target_layer_id),
                env!("CARGO_PKG_VERSION"),
            ) {
                Ok(r) => {
                    imported_filters_are_dither = {
                        fn has_dither(nodes: &[engine_project::LayerNode], layer_id: u32) -> bool {
                            for node in nodes {
                                match node {
                                    engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                                        return layer
                                            .filters
                                            .iter()
                                            .any(|f| matches!(f.params, FilterParams::DitherV2(_)));
                                    }
                                    engine_project::LayerNode::Group(g) => {
                                        if has_dither(&g.children, layer_id) {
                                            return true;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            false
                        }
                        has_dither(&doc.root, target_layer_id)
                    };
                    doc.increment_generation();
                    out = Some(r);
                }
                Err(e) => err = Some(e.to_string()),
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        let result = out.ok_or_else(|| "Import failed".to_string())?;

        if imported_filters_are_dither {
            state.tiles.error_residuals.evict_layer(
                doc_id,
                engine_project::types::LayerId::new(target_layer_id),
            );
            state.tiles.block_representatives.clear_dithered();
        }

        {
            let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
            snapshot
                .generations
                .increment_layer_gen(target_layer_id);
        }

        let doc = state.require_session(doc_id)?.document_handle.snapshot().id.0;
        engine_tiles::invalidation::invalidate(
            &state.tiles.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: target_layer_id,
            },
        );
        schedule_dirty_viewport_tiles(&state);
        emit_document_changed(&app_handle, "pattern_imported", Some(target_layer_id), Some(doc_id));

        Ok(ImportPatternResponse {
            filter_ids: result.filter_ids.iter().map(|id| id.to_string()).collect(),
            palette_ids: result.palette_ids.iter().map(|id| id.0).collect(),
        })
        })
        .map_err(AppError::Generic)?;
        Ok(result)
    }

    pub async fn export_image(&self, req: ExportImageRequest) -> Result<(), AppError> {
        use engine_project::filters::apply::apply_filter_to_tile;
        use engine_tiles::{TILE_SIZE, HALO, TileCoord, CacheStage, TileKey};
        use std::fs;
        use std::io::Cursor;

        if req.format != "PNG" && req.format != "JPEG" && req.format != "SVG" {
            return Err(AppError::Generic("Invalid parameters: format must be PNG, JPEG, or SVG".to_string()));
        }

        let session = self.state.session(req.doc_id).map_err(|_| {
            AppError::Generic(format!(
                "Document was closed (id {}); cannot export",
                req.doc_id
            ))
        })?;
        let _io_guard = session.begin_io();
        let snapshot = session.document_handle.snapshot();
        let img_width = snapshot.width;
        let img_height = snapshot.height;

        let cols = (img_width + TILE_SIZE - 1) / TILE_SIZE;
        let rows = (img_height + TILE_SIZE - 1) / TILE_SIZE;
        let layer_id = 1u32;

        let mut tiles: Vec<Vec<Arc<PixelTile>>> = Vec::with_capacity(rows as usize);
        for row in 0..rows {
            let mut row_tiles: Vec<Arc<PixelTile>> = Vec::with_capacity(cols as usize);
            for col in 0..cols {
                let key = TileKey {
                    doc: req.doc_id,
                    layer: layer_id,
                    coord: TileCoord { level: 0, x: col, y: row },
                    stage: CacheStage::Raw,
                };
                match self.state.tiles.tile_cache.get_entry(key) {
                    Some(tile) => row_tiles.push(tile),
                    None => {
                        return Err(AppError::Generic(format!(
                            "Cannot export: image tiles missing from memory for document {} layer {} — reopen the file",
                            req.doc_id, layer_id
                        )));
                    }
                }
            }
            tiles.push(row_tiles);
        }

        let layer_clone = find_first_visible_layer(&snapshot.root).cloned();
        let doc_snapshot = (*snapshot).clone();
        drop(snapshot);

        let req_format = req.format.clone();
        let req_path = req.path.clone();
        let req_quality = req.quality;
        let req_svg_algorithm = req.svg_algorithm.clone();
        let state_clone = Arc::clone(&self.state);

        tauri::async_runtime::spawn_blocking(move || {
            let mut rgba_buffer: Vec<u8> = vec![0u8; (img_width * img_height * 4) as usize];

            for row in 0..rows {
                for col in 0..cols {
                    let tile = &tiles[row as usize][col as usize];

                    let processed_tile = if let Some(ref layer) = layer_clone {
                        let coord = TileCoord { level: 0, x: col, y: row };
                        apply_filter_to_tile(
                            tile,
                            layer,
                            coord,
                            &state_clone.tiles.palette_cache,
                            &state_clone.tiles.palette_lut_cache,
                            &state_clone.tiles.threshold_cache,
                            &doc_snapshot,
                        )
                            .map_err(|e| format!("Render error: {:?}", e))?
                    } else {
                        let mut copy = engine_tiles::PixelTile::new();
                        for y in 0u32..260 {
                            for x in 0u32..260 {
                                for c in 0..4 {
                                    copy.set(x, y, c, tile.at(x, y, c));
                                }
                            }
                        }
                        copy
                    };

                    let tile_origin_x = col * TILE_SIZE;
                    let tile_origin_y = row * TILE_SIZE;

                    for ty in 0..TILE_SIZE {
                        let img_y = tile_origin_y + ty;
                        if img_y >= img_height {
                            break;
                        }
                        for tx in 0..TILE_SIZE {
                            let img_x = tile_origin_x + tx;
                            if img_x >= img_width {
                                break;
                            }
                            let tile_x = tx + HALO;
                            let tile_y = ty + HALO;
                            let buf_idx = ((img_y * img_width + img_x) * 4) as usize;

                            rgba_buffer[buf_idx] = f32_to_u8(processed_tile.at(tile_x, tile_y, 0));
                            rgba_buffer[buf_idx + 1] = f32_to_u8(processed_tile.at(tile_x, tile_y, 1));
                            rgba_buffer[buf_idx + 2] = f32_to_u8(processed_tile.at(tile_x, tile_y, 2));
                            rgba_buffer[buf_idx + 3] = f32_to_u8(processed_tile.at(tile_x, tile_y, 3));
                        }
                    }
                }
            }

            match req_format.as_str() {
                "PNG" => {
                    let png_bytes = encode_rgba_to_png(&rgba_buffer, img_width, img_height)?;
                    fs::write(&req_path, &png_bytes)
                        .map_err(|e| format!("IO error: {}", e))?;
                }
                "JPEG" => {
                    use image::codecs::jpeg::JpegEncoder;
                    use image::ImageEncoder;

                    let mut rgb_buffer: Vec<u8> = Vec::with_capacity((img_width * img_height * 3) as usize);
                    for pixel in rgba_buffer.chunks_exact(4) {
                        rgb_buffer.push(pixel[0]);
                        rgb_buffer.push(pixel[1]);
                        rgb_buffer.push(pixel[2]);
                    }

                    let quality = req_quality.unwrap_or(90);
                    let mut jpeg_data: Vec<u8> = Vec::new();
                    let cursor = Cursor::new(&mut jpeg_data);
                    let encoder = JpegEncoder::new_with_quality(cursor, quality);
                    encoder
                        .write_image(&rgb_buffer, img_width, img_height, image::ExtendedColorType::Rgb8)
                        .map_err(|e| format!("JPEG encoding error: {}", e))?;

                    fs::write(&req_path, &jpeg_data)
                        .map_err(|e| format!("IO error: {}", e))?;
                }
                "SVG" => {
                    use engine_io::{write_svg_file, SvgAlgorithm, SvgExportOptions};
                    let algorithm = match req_svg_algorithm.as_deref() {
                        Some("contour_tracing") => SvgAlgorithm::ContourTracing,
                        _ => SvgAlgorithm::GreedyMeshing,
                    };
                    let opts = SvgExportOptions {
                        algorithm,
                        tolerance: 0,
                    };
                    write_svg_file(&req_path, img_width, img_height, &rgba_buffer, &opts)
                        .map_err(|e| format!("SVG export error: {}", e))?;
                }
                _ => unreachable!(),
            }

            Ok::<(), AppError>(())
        }).await.map_err(|e| AppError::Generic(format!("Export error: {}", e)))?
    }

    pub fn set_active_document(
        &self,
        doc_id: u32,
        app_handle: &AppHandle,
    ) -> Result<DocumentResponse, AppError> {
        self.state.activate(doc_id)?;
        emit_document_changed(app_handle, "document_activated", None, Some(doc_id));
        emit_tabs_changed(Some(app_handle), &self.state);
        schedule_dirty_viewport_tiles(&self.state);
        self.get_document_snapshot()
    }
}
