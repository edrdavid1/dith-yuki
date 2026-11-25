//! Typed `GamedevContext` persisted on the document (Foundation).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// JSON key used when bootstrapping context via `Document.extra` before the typed field.
pub const GAMEDEV_EXTRA_KEY: &str = "gamedev";

/// Alias for call sites that want a function form.
#[inline]
pub fn gamedev_extra_key() -> &'static str {
    GAMEDEV_EXTRA_KEY
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamedevJob {
    #[default]
    None,
    Tile,
    Sprite,
    Animation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TileSettings {
    /// Pipeline Tile (modifies pixels in F1+). Foundation stores the flag only.
    pub enabled: bool,
}

impl Default for TileSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CellSettings {
    pub w: u32,
    pub h: u32,
    pub padding: u32,
    pub independent: bool,
}

impl Default for CellSettings {
    fn default() -> Self {
        Self {
            w: 32,
            h: 32,
            padding: 0,
            independent: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteSource {
    PerImage,
    Shared { set_id: String },
    Fixed { palette_id: u32 },
}

impl Default for PaletteSource {
    fn default() -> Self {
        Self::PerImage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskPolicy {
    Free,
    Fixed {
        kind: MaskKind,
        phase: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskKind {
    Ordered,
    BlueNoise,
}

impl Default for MaskPolicy {
    fn default() -> Self {
        Self::Free
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExportDefaults {
    pub png8: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for ExportDefaults {
    fn default() -> Self {
        Self {
            png8: false,
            naming: None,
            folder: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AlphaPolicy {
    /// Alpha below this (0–1) becomes fully transparent when thresholding (F4).
    pub threshold: f32,
    pub no_dither_alpha: bool,
    pub clear_rgb_in_transparent: bool,
}

impl Default for AlphaPolicy {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            no_dither_alpha: true,
            clear_rgb_in_transparent: true,
        }
    }
}

impl PartialEq for AlphaPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.no_dither_alpha == other.no_dither_alpha
            && self.clear_rgb_in_transparent == other.clear_rgb_in_transparent
            && (self.threshold - other.threshold).abs() < f32::EPSILON
    }
}

impl Eq for AlphaPolicy {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PixelPerfect {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_res: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integer_upscale: Option<u32>,
    /// View-only: nearest-neighbor preview. Does not affect render cache.
    pub nearest_preview: bool,
}

impl Default for PixelPerfect {
    fn default() -> Self {
        Self {
            target_res: None,
            integer_upscale: None,
            nearest_preview: false,
        }
    }
}

/// Document-scoped gamedev workflow state (engine source of truth).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GamedevContext {
    #[serde(default)]
    pub job: GamedevJob,
    #[serde(default)]
    pub tile: TileSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<CellSettings>,
    #[serde(default)]
    pub palette: PaletteSource,
    #[serde(default)]
    pub threshold_mask: MaskPolicy,
    #[serde(default)]
    pub export: ExportDefaults,
    #[serde(default)]
    pub alpha: AlphaPolicy,
    #[serde(default)]
    pub pixel_perfect: PixelPerfect,
    /// F3: list of frame paths. Not a new document type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_set: Vec<PathBuf>,
}

impl Default for GamedevContext {
    fn default() -> Self {
        Self {
            job: GamedevJob::None,
            tile: TileSettings::default(),
            cell: None,
            palette: PaletteSource::default(),
            threshold_mask: MaskPolicy::default(),
            export: ExportDefaults::default(),
            alpha: AlphaPolicy::default(),
            pixel_perfect: PixelPerfect::default(),
            frame_set: Vec::new(),
        }
    }
}

impl GamedevContext {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Whether this context must be declared as format feature `gamedev-context`.
    pub fn requires_format_feature(&self) -> bool {
        !self.is_default()
    }

    /// F5: bias Gamedev defaults (PNG8, nearest preview, hard alpha). Safe to
    /// call on workspace enter — only fills preference defaults; does not
    /// clear tile/cell/frame_set.
    pub fn apply_workspace_defaults(&mut self) {
        self.export.png8 = true;
        self.pixel_perfect.nearest_preview = true;
        self.alpha.no_dither_alpha = true;
        self.alpha.clear_rgb_in_transparent = true;
    }
}

/// If `extra` holds a bootstrap `gamedev` object, parse it and remove the key.
pub fn lift_gamedev_from_extra(extra: &mut Map<String, Value>) -> Option<GamedevContext> {
    let value = extra.remove(GAMEDEV_EXTRA_KEY)?;
    serde_json::from_value(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_json() {
        let ctx = GamedevContext::default();
        let v = serde_json::to_value(&ctx).unwrap();
        let back: GamedevContext = serde_json::from_value(v).unwrap();
        assert_eq!(ctx, back);
        assert!(ctx.is_default());
    }

    #[test]
    fn lift_from_extra_consumes_key() {
        let mut extra = Map::new();
        let mut ctx = GamedevContext::default();
        ctx.export.png8 = true;
        extra.insert(
            GAMEDEV_EXTRA_KEY.into(),
            serde_json::to_value(&ctx).unwrap(),
        );
        let lifted = lift_gamedev_from_extra(&mut extra).unwrap();
        assert!(lifted.export.png8);
        assert!(!extra.contains_key(GAMEDEV_EXTRA_KEY));
    }
}
