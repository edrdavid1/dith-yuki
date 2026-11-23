//! Cache-layer invalidation contract for `GamedevContext` updates.
//!
//! Foundation returns the set but does **not** recalculate pixels. F1+ will act on it.

use serde::{Deserialize, Serialize};

use super::context::GamedevContext;

/// Layers that may need refresh after a context change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamedevCacheLayer {
    /// Preview / dither tile cache (pixel pipeline).
    Render,
    /// Export path that reads context (cells, tile, png8 defaults, etc.).
    Export,
    /// Runtime chip validation cache.
    Chips,
}

/// Diff `old` → `new` and return which caches are stale.
///
/// Non-pixel fields (export naming/folder/png8 preference, job, frame_set,
/// nearest_preview) must **not** include [`GamedevCacheLayer::Render`].
pub fn gamedev_cache_invalidation(
    old: &GamedevContext,
    new: &GamedevContext,
) -> Vec<GamedevCacheLayer> {
    if old == new {
        return Vec::new();
    }

    let mut out = Vec::new();

    let pixel_affecting = pixel_affecting_changed(old, new);
    if pixel_affecting {
        out.push(GamedevCacheLayer::Render);
        out.push(GamedevCacheLayer::Export);
    } else if export_surface_changed(old, new) {
        // Export defaults / frame_set change export UX, not the live render cache.
        out.push(GamedevCacheLayer::Export);
    }

    // Any context change can stale chip results (stub or real).
    out.push(GamedevCacheLayer::Chips);
    out
}

fn pixel_affecting_changed(old: &GamedevContext, new: &GamedevContext) -> bool {
    old.tile != new.tile
        || old.cell != new.cell
        || old.palette != new.palette
        || old.threshold_mask != new.threshold_mask
        || old.alpha != new.alpha
        || old.pixel_perfect.target_res != new.pixel_perfect.target_res
        || old.pixel_perfect.integer_upscale != new.pixel_perfect.integer_upscale
}

fn export_surface_changed(old: &GamedevContext, new: &GamedevContext) -> bool {
    old.export != new.export || old.frame_set != new.frame_set || old.job != new.job
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamedev::ExportDefaults;

    #[test]
    fn non_pixel_export_png8_does_not_invalidate_render() {
        let old = GamedevContext::default();
        let mut new = old.clone();
        new.export.png8 = true;
        let layers = gamedev_cache_invalidation(&old, &new);
        assert!(
            !layers.contains(&GamedevCacheLayer::Render),
            "export.png8 must not invalidate render cache: {layers:?}"
        );
        assert!(layers.contains(&GamedevCacheLayer::Chips));
        assert!(layers.contains(&GamedevCacheLayer::Export));
    }

    #[test]
    fn tile_enabled_marks_render() {
        let old = GamedevContext::default();
        let mut new = old.clone();
        new.tile.enabled = true;
        let layers = gamedev_cache_invalidation(&old, &new);
        assert!(layers.contains(&GamedevCacheLayer::Render));
        assert!(layers.contains(&GamedevCacheLayer::Export));
        assert!(layers.contains(&GamedevCacheLayer::Chips));
    }

    #[test]
    fn identical_context_invalidates_nothing() {
        let ctx = GamedevContext {
            export: ExportDefaults {
                png8: true,
                naming: Some("{name}_dithered.png".into()),
                folder: None,
            },
            ..GamedevContext::default()
        };
        assert!(gamedev_cache_invalidation(&ctx, &ctx).is_empty());
    }

    #[test]
    fn nearest_preview_only_invalidates_chips() {
        let old = GamedevContext::default();
        let mut new = old.clone();
        new.pixel_perfect.nearest_preview = true;
        let layers = gamedev_cache_invalidation(&old, &new);
        assert!(!layers.contains(&GamedevCacheLayer::Render));
        assert!(!layers.contains(&GamedevCacheLayer::Export));
        assert_eq!(layers, vec![GamedevCacheLayer::Chips]);
    }

    #[test]
    fn cell_settings_invalidate_render_f2() {
        let old = GamedevContext::default();
        let mut new = old.clone();
        new.cell = Some(crate::gamedev::CellSettings {
            w: 48,
            h: 48,
            padding: 0,
            independent: true,
        });
        let layers = gamedev_cache_invalidation(&old, &new);
        assert!(layers.contains(&GamedevCacheLayer::Render));
        assert!(layers.contains(&GamedevCacheLayer::Export));
        assert!(layers.contains(&GamedevCacheLayer::Chips));
    }
}
