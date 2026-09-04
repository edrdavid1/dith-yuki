//! Speculative GPU warm-up budget (A2). No wgpu.

use engine_tiles::TileCoord;

/// Slots to keep free so a visible L0 frame can still promote without
/// kicking prefetch-warmed tiles into stealing viewport VRAM.
///
/// One Processed slot per visible layer plus one Composite slot per tile.
pub fn viewport_vram_reserve(visible_l0: usize, layer_count: usize) -> u32 {
    let layers = layer_count.max(1);
    (visible_l0.saturating_mul(layers.saturating_add(1))) as u32
}

/// How many slots speculative warm-up may consume.
pub fn warmup_slot_budget(free_slots: u32, viewport_reserve: u32) -> u32 {
    free_slots.saturating_sub(viewport_reserve)
}

/// Approximate resident slots one L0 coord will occupy after filter + composite.
pub fn slots_per_warmup_coord(layer_count: usize) -> u32 {
    (layer_count.max(1) as u32).saturating_add(1)
}

/// Visible first (unless `skip_visible`), then prefetch. L0 only.
pub fn select_warmup_coords(
    visible: &[TileCoord],
    prefetch: &[TileCoord],
    skip_visible: bool,
) -> Vec<TileCoord> {
    let mut out = Vec::new();
    if !skip_visible {
        out.extend(visible.iter().copied().filter(|c| c.level == 0));
    }
    out.extend(prefetch.iter().copied().filter(|c| c.level == 0));
    out
}

pub fn cap_warmup_coords(coords: &[TileCoord], budget_slots: u32, per_coord: u32) -> Vec<TileCoord> {
    if per_coord == 0 || budget_slots == 0 {
        return Vec::new();
    }
    let n = (budget_slots / per_coord) as usize;
    coords.iter().copied().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: u32) -> TileCoord {
        TileCoord {
            level: 0,
            x,
            y: 0,
        }
    }

    #[test]
    fn reserve_and_budget() {
        assert_eq!(viewport_vram_reserve(40, 2), 120);
        assert_eq!(warmup_slot_budget(118, 80), 38);
        assert_eq!(warmup_slot_budget(50, 80), 0);
        assert_eq!(slots_per_warmup_coord(2), 3);
    }

    #[test]
    fn select_skips_visible_when_opt_in_owns_them() {
        let vis = [c(0), TileCoord { level: 1, x: 0, y: 0 }];
        let pre = [c(1)];
        let all = select_warmup_coords(&vis, &pre, false);
        assert_eq!(all, vec![c(0), c(1)]);
        let pre_only = select_warmup_coords(&vis, &pre, true);
        assert_eq!(pre_only, vec![c(1)]);
    }

    #[test]
    fn cap_respects_budget() {
        let coords = [c(0), c(1), c(2), c(3)];
        assert_eq!(cap_warmup_coords(&coords, 6, 2).len(), 3);
        assert!(cap_warmup_coords(&coords, 0, 2).is_empty());
    }
}
