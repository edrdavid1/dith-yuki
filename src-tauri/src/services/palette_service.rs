use std::sync::Arc;
use tauri::AppHandle;

use serde::{Deserialize, Serialize};

use crate::commands::{emit_document_changed, schedule_dirty_viewport_tiles, AppState};
use crate::services::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct PaletteDto {
    pub id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
    pub hex_colors: Vec<String>,
    pub color_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddPaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub target_count: u16,
    pub method: String,
    #[serde(default)]
    pub chroma_weight: f32,
    #[serde(default)]
    pub contrast_weight: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinPaletteDto {
    pub id: String,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
    pub color_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplacePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenamePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportPaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub hex: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub index: usize,
    pub hex: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoveColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub from_index: usize,
    pub to_index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePaletteResponse {
    pub affected_filter_ids: Vec<String>,
}

pub fn hex_to_linear(hex: &str) -> Result<engine_color::palette::LinearColor, String> {
    use engine_color::palette::{srgb_to_linear, LinearColor};

    if hex.len() != 6 {
        return Err("Hex color must be exactly 6 characters".to_string());
    }
    let r = u8::from_str_radix(&hex[0..2], 16)
        .map_err(|_| "Invalid hex character in red channel".to_string())?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|_| "Invalid hex character in green channel".to_string())?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .map_err(|_| "Invalid hex character in blue channel".to_string())?;
    Ok(LinearColor {
        r: srgb_to_linear(r),
        g: srgb_to_linear(g),
        b: srgb_to_linear(b),
    })
}

pub fn linear_to_hex(color: &engine_color::palette::LinearColor) -> String {
    use engine_color::palette::linear_to_srgb;

    let r = linear_to_srgb(color.r);
    let g = linear_to_srgb(color.g);
    let b = linear_to_srgb(color.b);
    format!("{:02X}{:02X}{:02X}", r, g, b)
}

pub fn find_layers_referencing_palette(
    nodes: &[engine_project::layer::LayerNode],
    palette_id: engine_project::types::PaletteId,
) -> Vec<engine_project::types::LayerId> {
    use engine_project::filter::FilterParams;
    use engine_project::layer::LayerNode;

    let mut result = Vec::new();
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                let references_palette = layer.filters.iter().any(|filter| {
                    match &filter.params {
                        FilterParams::DitherV2(params) => params.palette_id == Some(palette_id),
                        FilterParams::PaletteQuantize { palette_id: pid, .. } => *pid == palette_id,
                        _ => false,
                    }
                });
                if references_palette {
                    result.push(layer.id);
                }
            }
            LayerNode::Group(group) => {
                let mut child_results = find_layers_referencing_palette(&group.children, palette_id);
                result.append(&mut child_results);
            }
        }
    }
    result
}

fn invalidate_palette_changed(palette_id: engine_project::types::PaletteId, state: &AppState) {
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let affected_layers = find_layers_referencing_palette(&snapshot.root, palette_id);

    for layer_id in &affected_layers {
        engine_tiles::invalidation::invalidate(
            &state.tiles.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: snapshot.id.0, layer: layer_id.0,
            },
        );
    }

    if !affected_layers.is_empty() {
        schedule_dirty_viewport_tiles(state);
    }
}

pub fn palette_to_dto(palette: &engine_color::palette::Palette) -> PaletteDto {
    use engine_color::palette::linear_to_srgb;
    let colors: Vec<[u8; 3]> = palette
        .colors
        .iter()
        .map(|c| [linear_to_srgb(c.r), linear_to_srgb(c.g), linear_to_srgb(c.b)])
        .collect();
    let hex_colors: Vec<String> = palette
        .colors
        .iter()
        .map(|c| linear_to_hex(c))
        .collect();
    let color_count = colors.len();
    PaletteDto {
        id: palette.id,
        name: palette.name.clone(),
        colors,
        hex_colors,
        color_count,
    }
}

pub struct PaletteService {
    state: Arc<AppState>,
}

impl PaletteService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn list_palettes(&self) -> Result<Vec<PaletteDto>, AppError> {
        let Ok(session) = self.state.active_session() else {
            return Ok(vec![]);
        };
        let snapshot = session.document_handle.snapshot();
        let dtos: Vec<PaletteDto> = snapshot.palettes.iter().map(palette_to_dto).collect();
        Ok(dtos)
    }

    pub fn list_builtin_palettes() -> Result<Vec<BuiltinPaletteDto>, AppError> {
        use engine_color::palette::BUILTIN_PRESETS;

        Ok(BUILTIN_PRESETS
            .iter()
            .map(|p| BuiltinPaletteDto {
                id: p.id.to_string(),
                name: p.name.to_string(),
                colors: p
                    .colors_srgb
                    .iter()
                    .map(|&(r, g, b)| [r, g, b])
                    .collect(),
                color_count: p.colors_srgb.len(),
            })
            .collect())
    }

    pub fn import_builtin_palette(
        &self,
        app_handle: &AppHandle,
        doc_id: u32,
        id: String,
    ) -> Result<PaletteDto, AppError> {
        use engine_color::palette::{find_preset, srgb_to_linear, LinearColor};

        let preset = find_preset(&id).ok_or_else(|| {
            AppError::Generic(format!(
                "Unknown builtin palette id '{}'. Use list_builtin_palettes for valid ids.",
                id
            ))
        })?;

        let linear_colors: Vec<LinearColor> = preset
            .colors_srgb
            .iter()
            .map(|&(r, g, b)| LinearColor {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            })
            .collect();

        let mut palette_id_raw = 0u32;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                let pid = doc.add_palette(preset.name.to_string(), linear_colors);
                palette_id_raw = pid.0;
                doc.increment_generation();
            });

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == palette_id_raw)
                .ok_or_else(|| "Failed to find newly imported builtin palette".to_string())?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn import_palette(
        &self,
        app_handle: &AppHandle,
        doc_id: u32,
        path: String,
    ) -> Result<PaletteDto, AppError> {
        use engine_color::palette::{import_palette as do_import, PaletteFormat};
        use std::path::Path;

        let file_path = Path::new(&path);
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let format = match ext.as_str() {
            "ase" => PaletteFormat::Ase,
            "aco" => PaletteFormat::Aco,
            "gpl" => PaletteFormat::Gpl,
            "pal" => PaletteFormat::Pal,
            "csv" => PaletteFormat::Csv,
            "json" => PaletteFormat::Json,
            _ => return Err(AppError::Generic(format!("Unsupported palette format: .{}", ext))),
        };

        let linear_colors = do_import(file_path, format).map_err(|e| AppError::Generic(format!("{}", e)))?;

        let name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .to_string();

        let mut palette_id_raw = 0u32;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                let pid = doc.add_palette(name.clone(), linear_colors.clone());
                palette_id_raw = pid.0;
                doc.increment_generation();
            });

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == palette_id_raw)
                .ok_or_else(|| "Failed to find newly added palette".to_string())?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn add_palette(
        &self,
        app_handle: &AppHandle,
        req: AddPaletteRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_color::palette::{srgb_to_linear, LinearColor};

        let linear_colors: Vec<LinearColor> = req
            .colors
            .iter()
            .map(|[r, g, b]| LinearColor {
                r: srgb_to_linear(*r),
                g: srgb_to_linear(*g),
                b: srgb_to_linear(*b),
            })
            .collect();

        let mut palette_id_raw = 0u32;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                let pid = doc.add_palette(req.name.clone(), linear_colors);
                palette_id_raw = pid.0;
                doc.increment_generation();
            });

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == palette_id_raw)
                .ok_or_else(|| "Failed to find newly added palette".to_string())?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn replace_palette(
        &self,
        app_handle: &AppHandle,
        req: ReplacePaletteRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_color::palette::{srgb_to_linear, LinearColor};
        use engine_project::types::PaletteId;

        let trimmed = req.name.trim().to_string();
        if trimmed.is_empty() || trimmed.len() > 255 {
            return Err(AppError::InvalidOperation("Name must be 1–255 characters".to_string()));
        }
        if req.colors.is_empty() {
            return Err(AppError::InvalidOperation("Palette must contain at least one color".to_string()));
        }

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            if !snapshot.palettes.iter().any(|p| p.id == req.palette_id) {
                return Err(AppError::Generic(format!("Palette {} not found", req.palette_id)));
            }
        }

        let linear_colors: Vec<LinearColor> = req
            .colors
            .iter()
            .map(|[r, g, b]| LinearColor {
                r: srgb_to_linear(*r),
                g: srgb_to_linear(*g),
                b: srgb_to_linear(*b),
            })
            .collect();

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                let _ = doc.modify_palette(PaletteId::new(req.palette_id), linear_colors.clone());
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    palette.name = trimmed.clone();
                }
                doc.increment_generation();
            });

            invalidate_palette_changed(PaletteId::new(req.palette_id), &self.state);

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn remove_palette(
        &self,
        app_handle: &AppHandle,
        doc_id: u32,
        palette_id: u32,
    ) -> Result<(), AppError> {
        use engine_project::types::PaletteId;

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let mut result: Result<(), String> = Ok(());
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                match doc.remove_palette(PaletteId::new(palette_id)) {
                    Ok(_) => {
                        doc.increment_generation();
                    }
                    Err(e) => {
                        result = Err(format!("{}", e));
                    }
                }
            });
            result
        })
        .map_err(AppError::Generic)
    }

    pub fn rename_palette(
        &self,
        app_handle: &AppHandle,
        req: RenamePaletteRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        let trimmed_name = req.name.trim().to_string();
        if trimmed_name.is_empty() || trimmed_name.len() > 255 {
            return Err(AppError::InvalidOperation("Name must be 1–255 characters".to_string()));
        }

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            if !snapshot.palettes.iter().any(|p| p.id == req.palette_id) {
                return Err(AppError::Generic(format!("Palette {} not found", req.palette_id)));
            }
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    palette.name = trimmed_name;
                }
            });

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn create_palette(
        &self,
        app_handle: &AppHandle,
        req: CreatePaletteRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        let trimmed_name = req.name.trim().to_string();
        if trimmed_name.is_empty() || trimmed_name.len() > 255 {
            return Err(AppError::InvalidOperation("Name must be 1–255 characters".to_string()));
        }

        let mut palette_id_raw = 0u32;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                let pid = doc.add_palette(trimmed_name.clone(), vec![]);
                palette_id_raw = pid.0;
                doc.increment_generation();
            });

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == palette_id_raw)
                .ok_or_else(|| "Failed to find newly created palette".to_string())?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn export_palette(&self, req: ExportPaletteRequest) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        use engine_color::palette::{export_palette as do_export, PaletteFormat};

        let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;

        if palette.colors.is_empty() {
            return Err(AppError::InvalidOperation("Palette is empty and cannot be exported".to_string()));
        }

        let format = match req.format.to_lowercase().as_str() {
            "ase" => PaletteFormat::Ase,
            "aco" => PaletteFormat::Aco,
            "gpl" => PaletteFormat::Gpl,
            "pal" => PaletteFormat::Pal,
            "csv" => PaletteFormat::Csv,
            "json" => PaletteFormat::Json,
            _ => return Err(AppError::Generic(format!("Unsupported export format: {}", req.format))),
        };

        let bytes = do_export(palette, format).map_err(|e| AppError::Generic(format!("{}", e)))?;

        std::fs::write(&req.path, &bytes)
            .map_err(|e| AppError::Generic(format!("Failed to write file: {}", e)))?;

        Ok(())
    }

    pub fn add_color_to_palette(
        &self,
        app_handle: &AppHandle,
        req: AddColorRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_project::types::PaletteId;

        let color = hex_to_linear(&req.hex)?;

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;
            if palette.colors.len() >= 65536 {
                return Err(AppError::InvalidOperation("Palette has reached maximum size (65536 colors)".to_string()));
            }
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    palette.colors.push(color);
                    palette.revision += 1;
                }
            });

            invalidate_palette_changed(PaletteId::new(req.palette_id), &self.state);

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn update_palette_color(
        &self,
        app_handle: &AppHandle,
        req: UpdateColorRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_project::types::PaletteId;

        let color = hex_to_linear(&req.hex)?;

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;

            let color_count = palette.colors.len();
            if req.index >= color_count {
                return Err(AppError::Generic(format!(
                    "Color index {} out of bounds (palette has {} colors)",
                    req.index, color_count
                )));
            }
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    palette.colors[req.index] = color;
                    palette.revision += 1;
                }
            });

            invalidate_palette_changed(PaletteId::new(req.palette_id), &self.state);

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn remove_palette_color(
        &self,
        app_handle: &AppHandle,
        req: RemoveColorRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_project::types::PaletteId;

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;

            let color_count = palette.colors.len();
            if req.index >= color_count {
                return Err(AppError::Generic(format!(
                    "Color index {} out of bounds (palette has {} colors)",
                    req.index, color_count
                )));
            }

            if color_count == 1 {
                let referencing_layers =
                    find_layers_referencing_palette(&snapshot.root, PaletteId::new(req.palette_id));
                if !referencing_layers.is_empty() {
                    return Err(AppError::InvalidOperation(
                        "Cannot remove last color from a palette referenced by filters".to_string(),
                    ));
                }
            }
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    palette.colors.remove(req.index);
                    palette.revision += 1;
                }
            });

            invalidate_palette_changed(PaletteId::new(req.palette_id), &self.state);

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn reorder_palette_color(
        &self,
        app_handle: &AppHandle,
        req: ReorderColorRequest,
    ) -> Result<PaletteDto, AppError> {
        let doc_id = req.doc_id;
        use engine_project::types::PaletteId;

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;

            let color_count = palette.colors.len();
            if req.from_index >= color_count || req.to_index >= color_count {
                return Err(AppError::Generic("Index out of bounds".to_string()));
            }
        }

        if req.from_index == req.to_index {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| AppError::Generic(format!("Palette {} not found", req.palette_id)))?;
            return Ok(palette_to_dto(palette));
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                    let color = palette.colors.remove(req.from_index);
                    palette.colors.insert(req.to_index, color);
                    palette.revision += 1;
                }
            });

            invalidate_palette_changed(PaletteId::new(req.palette_id), &self.state);

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == req.palette_id)
                .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }

    pub fn delete_palette(
        &self,
        app_handle: &AppHandle,
        doc_id: u32,
        palette_id: u32,
    ) -> Result<DeletePaletteResponse, AppError> {
        use engine_project::types::PaletteId;

        let pid = PaletteId::new(palette_id);

        {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            if !snapshot.palettes.iter().any(|p| p.id == palette_id) {
                return Err(AppError::Generic(format!("Palette {} not found", palette_id)));
            }
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let mut affected_filter_ids: Vec<String> = Vec::new();
            let mut affected_layer_ids: Vec<u32> = Vec::new();

            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                fn clear_palette_refs(
                    nodes: &mut Vec<engine_project::layer::LayerNode>,
                    palette_id: engine_project::types::PaletteId,
                    affected_filter_ids: &mut Vec<String>,
                    affected_layer_ids: &mut Vec<u32>,
                ) {
                    for node in nodes.iter_mut() {
                        match node {
                            engine_project::layer::LayerNode::Leaf(layer) => {
                                let mut layer_affected = false;
                                let mut filters_to_remove: Vec<usize> = Vec::new();

                                for (idx, filter) in layer.filters.iter_mut().enumerate() {
                                    match &mut filter.params {
                                        engine_project::filter::FilterParams::DitherV2(params) => {
                                            if params.palette_id == Some(palette_id) {
                                                params.palette_id = None;
                                                affected_filter_ids.push(filter.id.to_string());
                                                layer_affected = true;
                                            }
                                        }
                                        engine_project::filter::FilterParams::PaletteQuantize { palette_id: pid, .. } => {
                                            if *pid == palette_id {
                                                affected_filter_ids.push(filter.id.to_string());
                                                filters_to_remove.push(idx);
                                                layer_affected = true;
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                for idx in filters_to_remove.into_iter().rev() {
                                    layer.filters.remove(idx);
                                }

                                if layer_affected {
                                    affected_layer_ids.push(layer.id.0);
                                }
                            }
                            engine_project::layer::LayerNode::Group(group) => {
                                clear_palette_refs(
                                    &mut group.children,
                                    palette_id,
                                    affected_filter_ids,
                                    affected_layer_ids,
                                );
                            }
                        }
                    }
                }

                clear_palette_refs(&mut doc.root, pid, &mut affected_filter_ids, &mut affected_layer_ids);

                doc.palettes.retain(|p| p.id != palette_id);

                doc.increment_generation();
            });

            let doc = self.state.require_session(doc_id)?.document_handle.snapshot().id.0;
            self.state.tiles.palette_cache.evict(doc, palette_id);
            self.state.tiles.palette_lut_cache.evict(doc, palette_id);

            for layer_id in &affected_layer_ids {
                engine_tiles::invalidation::invalidate(
                    &self.state.tiles.tile_cache,
                    engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: *layer_id,
                    },
                );
            }

            if !affected_layer_ids.is_empty() {
                schedule_dirty_viewport_tiles(&self.state);
            }

            Ok(DeletePaletteResponse { affected_filter_ids })
        })
        .map_err(AppError::Generic)
    }

    pub fn generate_palette_blocking(
        &self,
        req: GeneratePaletteRequest,
        app: Option<&AppHandle>,
    ) -> Result<PaletteDto, AppError> {
        use engine_color::palette::generate::{PaletteGenMethod, MAX_GENERATION_SAMPLES};
        use engine_color::palette::LinearColor;
        use engine_tiles::{CacheStage, TileCoord, TileKey, HALO, TILE_SIZE};

        let doc_id = req.doc_id;
        let method = match req.method.as_str() {
            "KMeans" => PaletteGenMethod::KMeans,
            _ => PaletteGenMethod::MedianCut,
        };

        if req.target_count < 2 || req.target_count > 256 {
            return Err(AppError::InvalidOperation("target_count must be between 2 and 256".to_string()));
        }

        let weights = engine_color::palette::generate::GenerateWeights {
            chroma_weight: req.chroma_weight,
            contrast_weight: req.contrast_weight,
        };
        weights
            .validated()
            .map_err(|e| AppError::Generic(e.to_string()))?;

        let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
        let doc_width = snapshot.width;
        let doc_height = snapshot.height;
        drop(snapshot);

        let total_pixels = (doc_width as u64).saturating_mul(doc_height as u64).max(1);
        let stride =
            ((total_pixels as usize + MAX_GENERATION_SAMPLES - 1) / MAX_GENERATION_SAMPLES).max(1);

        let cols = (doc_width + TILE_SIZE - 1) / TILE_SIZE;
        let rows = (doc_height + TILE_SIZE - 1) / TILE_SIZE;

        let mut pixels: Vec<(LinearColor, f32)> =
            Vec::with_capacity((total_pixels as usize / stride).min(MAX_GENERATION_SAMPLES + 1024));
        let mut sample_index: u64 = 0;

        for row in 0..rows {
            for col in 0..cols {
                let key = TileKey {
                    doc: doc_id,
                    layer: req.layer_id,
                    coord: TileCoord {
                        level: 0,
                        x: col,
                        y: row,
                    },
                    stage: CacheStage::Raw,
                };
                if let Some(tile) = self.state.tiles.tile_cache.get_entry(key) {
                    let tile_max_x =
                        std::cmp::min(TILE_SIZE, doc_width.saturating_sub(col * TILE_SIZE));
                    let tile_max_y =
                        std::cmp::min(TILE_SIZE, doc_height.saturating_sub(row * TILE_SIZE));
                    for ty in 0..tile_max_y {
                        for tx in 0..tile_max_x {
                            let take = sample_index % stride as u64 == 0;
                            sample_index += 1;
                            if !take {
                                continue;
                            }
                            let px = tx + HALO;
                            let py = ty + HALO;
                            let a = tile.at(px, py, 3);
                            if a <= 0.0 {
                                continue;
                            }
                            pixels.push((
                                LinearColor {
                                    r: tile.at(px, py, 0),
                                    g: tile.at(px, py, 1),
                                    b: tile.at(px, py, 2),
                                },
                                a,
                            ));
                        }
                    }
                }
            }
        }

        if pixels.is_empty() {
            return Err(AppError::InvalidOperation("No tile data available for this layer. Load an image first.".to_string()));
        }

        let mut palette_id_raw = 0u32;
        crate::undo::with_document_undo(&self.state, app, doc_id, || {
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                match engine_project::palette_gen::generate_palette_from_layer_weighted(
                    doc,
                    engine_project::types::LayerId::new(req.layer_id),
                    pixels.into_iter(),
                    req.target_count,
                    method,
                    weights,
                ) {
                    Ok(pid) => {
                        palette_id_raw = pid.0;
                        doc.increment_generation();
                    }
                    Err(_) => {}
                }
            });

            if palette_id_raw == 0 {
                return Err(
                    "Palette generation failed. Ensure the layer has non-transparent pixels.".to_string(),
                );
            }

            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let palette = snapshot
                .palettes
                .iter()
                .find(|p| p.id == palette_id_raw)
                .ok_or_else(|| "Failed to find generated palette".to_string())?;
            Ok(palette_to_dto(palette))
        })
        .map_err(AppError::Generic)
    }
}
