//! Palette-derived channel ranges and Guided quantize (Track Q).

use dashmap::DashMap;
use std::sync::Arc;

use crate::palette::{Palette, PaletteId};

/// Linear-RGB min/max for one channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelRange {
    pub min: f32,
    pub max: f32,
}

impl ChannelRange {
    pub const UNIT: Self = Self { min: 0.0, max: 1.0 };
}

/// Min/max per linear RGB channel over `palette.colors`.
/// Empty or degenerate (min==max) channels fall back to `[0, 1]`.
pub fn palette_channel_ranges(palette: &Palette) -> [ChannelRange; 3] {
    if palette.colors.is_empty() {
        return [ChannelRange::UNIT; 3];
    }
    let mut mins = [f32::MAX; 3];
    let mut maxs = [f32::MIN; 3];
    for c in &palette.colors {
        let ch = [c.r, c.g, c.b];
        for i in 0..3 {
            mins[i] = mins[i].min(ch[i]);
            maxs[i] = maxs[i].max(ch[i]);
        }
    }
    let mut out = [ChannelRange::UNIT; 3];
    for i in 0..3 {
        if (maxs[i] - mins[i]).abs() < 1e-6 {
            out[i] = ChannelRange::UNIT;
        } else {
            out[i] = ChannelRange {
                min: mins[i],
                max: maxs[i],
            };
        }
    }
    out
}

/// `ceil(cbrt(N)).clamp(2, 16)` with `N = palette.colors.len().max(1)`.
pub fn default_channel_levels(palette: &Palette) -> u8 {
    let n = palette.colors.len().max(1) as f32;
    n.cbrt().ceil().clamp(2.0, 16.0) as u8
}

/// Per-channel ordered/ED quantize into a palette-derived range.
/// Shared `threshold` (Bayer or 0.5 for ED) for R, G, and B.
pub fn quantize_channel_guided(
    value: f32,
    range: ChannelRange,
    levels: u8,
    threshold: f32,
) -> f32 {
    let levels = levels.max(2) as f32;
    let span = (range.max - range.min).max(1e-6);
    let normalized = ((value - range.min) / span).clamp(0.0, 1.0);
    let scaled = normalized * (levels - 1.0);
    let base = scaled.floor();
    let frac = scaled - base;
    let step = if frac > threshold { base + 1.0 } else { base };
    let step = step.clamp(0.0, levels - 1.0);
    range.min + (step / (levels - 1.0)) * span
}

/// Revision-keyed cache of [`palette_channel_ranges`], scoped by document.
pub struct PaletteChannelRangeCache {
    entries: DashMap<(u32, PaletteId), (u64, Arc<[ChannelRange; 3]>)>,
}

impl PaletteChannelRangeCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    pub fn get_or_compute(&self, doc_id: u32, palette: &Palette) -> [ChannelRange; 3] {
        let key = (doc_id, palette.id);
        if let Some(entry) = self.entries.get(&key) {
            let (rev, ref ranges) = *entry;
            if rev == palette.revision {
                return **ranges;
            }
        }
        let ranges = palette_channel_ranges(palette);
        self.entries
            .insert(key, (palette.revision, Arc::new(ranges)));
        ranges
    }

    pub fn evict(&self, doc_id: u32, palette_id: PaletteId) {
        self.entries.remove(&(doc_id, palette_id));
    }

    pub fn evict_document(&self, doc_id: u32) {
        self.entries.retain(|&(doc, _), _| doc != doc_id);
    }
}

impl Default for PaletteChannelRangeCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::LinearColor;

    fn pal(n: usize) -> Palette {
        Palette {
            id: 1,
            name: "t".into(),
            colors: (0..n)
                .map(|i| LinearColor {
                    r: i as f32 / n as f32,
                    g: 0.0,
                    b: 0.0,
                })
                .collect(),
            revision: 1,
        }
    }

    #[test]
    fn guided_channel_levels_default_matches_cbrt_formula() {
        assert_eq!(default_channel_levels(&pal(4)), 2);
        assert_eq!(default_channel_levels(&pal(16)), 3);
        assert_eq!(default_channel_levels(&pal(64)), 4);
    }

    #[test]
    fn empty_palette_ranges_unit() {
        let p = Palette {
            id: 1,
            name: "e".into(),
            colors: vec![],
            revision: 1,
        };
        assert_eq!(palette_channel_ranges(&p), [ChannelRange::UNIT; 3]);
    }
}
