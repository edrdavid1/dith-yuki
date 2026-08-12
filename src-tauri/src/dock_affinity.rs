//! Dock Affinity: hit-test + float-drag session for drag-to-redock.
//!
//! Pure geometry lives here so it can be unit-tested without Tauri IPC.
//! Affinity arms only near horizontal insert gaps (same cue as docked reorder),
//! not over the full vertical sidebar column.
//!
//! Supports two dock zones (left + right); at most one zone is armed at a time.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Height of the floating titlebar band used as the hit-test anchor (logical px).
pub const HIT_BAND_HEIGHT: f64 = 32.0;
/// Horizontal width of the hit-test band, centered on the floating window (logical px).
pub const HIT_BAND_WIDTH: f64 = 64.0;
/// Half-height of the horizontal insert-gap hit strip on enter (logical px).
pub const GAP_HIT_HALF_ENTER: f64 = 14.0;
/// Half-height of the insert-gap hit strip while already armed (logical px).
pub const GAP_HIT_HALF_EXIT: f64 = 28.0;
/// Extra horizontal padding around the dock zone when deciding exit (logical px).
pub const HYSTERESIS_EXIT_PADDING: f64 = 24.0;

/// Panels that never participate in drag-to-redock.
pub const FLOATING_ONLY_PANELS: &[&str] = &["preview", "preferences"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockSlot {
    /// Vertical midpoint of a docked panel slot in the same coordinate space as the zone.
    pub mid_y: f64,
    /// Top edge of the panel slot (logical px). Optional for older callers.
    #[serde(default)]
    pub top: f64,
    /// Bottom edge of the panel slot (logical px). Optional for older callers.
    #[serde(default)]
    pub bottom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockZone {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_scale")]
    pub scale_factor: f64,
    pub side: SidebarSide,
    #[serde(default)]
    pub slots: Vec<DockSlot>,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center_y(self) -> f64 {
        self.y + self.height * 0.5
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn inflate(self, pad: f64) -> Rect {
        Rect {
            x: self.x - pad,
            y: self.y - pad,
            width: self.width + pad * 2.0,
            height: self.height + pad * 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FloatDragSession {
    pub panel_id: String,
    pub armed: bool,
    pub insert_index: usize,
    pub inside: bool,
    pub armed_side: Option<SidebarSide>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DockAffinityEvent {
    pub panel_id: String,
    pub armed: bool,
    pub insert_index: Option<usize>,
    /// Armed dock side when `armed`; null when disarmed.
    pub side: Option<SidebarSide>,
}

#[derive(Debug, Default)]
pub struct DockAffinityController {
    pub enabled: bool,
    /// Active dock zones keyed by side (at most one entry per side).
    pub zones: HashMap<SidebarSide, DockZone>,
    pub session: Option<FloatDragSession>,
    last_emitted: Option<(bool, usize, Option<SidebarSide>)>,
}

impl DockAffinityController {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            zones: HashMap::new(),
            session: None,
            last_emitted: None,
        }
    }

    pub fn is_floating_only(panel_id: &str) -> bool {
        FLOATING_ONLY_PANELS.contains(&panel_id)
    }

    /// Upsert or clear the zone for `side`. Invalid (zero-size) zones clear that side.
    pub fn set_dock_zone(&mut self, side: SidebarSide, zone: Option<DockZone>) {
        match zone {
            Some(mut z) if z.width > 0.0 && z.height > 0.0 => {
                z.side = side;
                self.zones.insert(side, z);
            }
            _ => {
                self.zones.remove(&side);
            }
        }
    }

    /// Start a float-drag session. Returns false if rejected (disabled / floating-only).
    pub fn begin(&mut self, panel_id: &str) -> bool {
        if !self.enabled || Self::is_floating_only(panel_id) {
            return false;
        }
        self.session = Some(FloatDragSession {
            panel_id: panel_id.to_string(),
            armed: false,
            insert_index: 0,
            inside: false,
            armed_side: None,
        });
        self.last_emitted = None;
        true
    }

    /// End session. Returns the final disarm event payload when a session existed.
    pub fn end_session(&mut self) -> Option<DockAffinityEvent> {
        let session = self.session.take()?;
        self.last_emitted = None;
        Some(DockAffinityEvent {
            panel_id: session.panel_id,
            armed: false,
            insert_index: None,
            side: None,
        })
    }

    pub fn cancel(&mut self) -> Option<DockAffinityEvent> {
        self.end_session()
    }

    /// Update affinity from the floating window outer rect (logical px).
    /// Returns an event only when (armed, insert_index, side) changes.
    pub fn on_moved(&mut self, win: Rect) -> Option<DockAffinityEvent> {
        let session = self.session.as_mut()?;
        let prev_inside = session.inside;
        let prev_side = session.armed_side;
        let (inside, armed, insert, side) =
            update_affinity_multi(&self.zones, win, prev_inside, prev_side);
        session.inside = inside;
        session.armed = armed;
        session.insert_index = insert;
        session.armed_side = side;

        let key = (armed, insert, side);
        if self.last_emitted.as_ref() == Some(&key) {
            return None;
        }
        self.last_emitted = Some(key);
        eprintln!(
            "[dock-affinity] on_moved win=({:.0},{:.0},{:.0}x{:.0}) inside={} armed={} insert={} side={:?}",
            win.x, win.y, win.width, win.height, inside, armed, insert, side
        );
        Some(DockAffinityEvent {
            panel_id: session.panel_id.clone(),
            armed,
            insert_index: if armed { Some(insert) } else { None },
            side: if armed { side } else { None },
        })
    }

    /// Snapshot of current session for mouseup completion: (panel_id, armed, insert, side).
    pub fn session_snapshot(&self) -> Option<(String, bool, usize, Option<SidebarSide>)> {
        self.session.as_ref().map(|s| {
            (
                s.panel_id.clone(),
                s.armed,
                s.insert_index,
                s.armed_side,
            )
        })
    }
}

pub fn titlebar_band(win: Rect) -> Rect {
    let height = HIT_BAND_HEIGHT.min(win.height.max(0.0));
    let width = HIT_BAND_WIDTH.min(win.width.max(0.0));
    let x = win.x + ((win.width - width) * 0.5).max(0.0);
    Rect {
        x,
        y: win.y,
        width,
        height,
    }
}

/// Titlebar probe aligned to the left or right edge of the floating window.
/// Used for vacant dock sides so a wide float can still hit a thin edge strip.
pub fn side_titlebar_band(win: Rect, side: SidebarSide) -> Rect {
    let height = HIT_BAND_HEIGHT.min(win.height.max(0.0));
    let width = HIT_BAND_WIDTH.min(win.width.max(0.0));
    let x = match side {
        SidebarSide::Left => win.x,
        SidebarSide::Right => win.x + (win.width - width).max(0.0),
    };
    Rect {
        x,
        y: win.y,
        width,
        height,
    }
}

fn probe_band_for_side(win: Rect, side: SidebarSide, empty_zone: bool) -> Rect {
    if empty_zone {
        side_titlebar_band(win, side)
    } else {
        titlebar_band(win)
    }
}

fn probe_band_for_zone(win: Rect, zone: &DockZone) -> Rect {
    probe_band_for_side(win, zone.side, zone.slots.is_empty())
}

/// Same midpoint semantics as frontend `computeInsertIndex`.
pub fn compute_insert_index(panel_mids: &[f64], pointer_y: f64) -> usize {
    if panel_mids.is_empty() {
        return 0;
    }
    let mut idx = 0usize;
    for &mp in panel_mids {
        if pointer_y >= mp {
            idx += 1;
        }
    }
    idx
}

/// Horizontal insert-gap Y positions (before / between / after docked panels).
pub fn insert_gap_ys(zone: &DockZone) -> Vec<f64> {
    if zone.slots.is_empty() {
        // Empty stack: single landing line in the middle of the zone.
        return vec![zone.y + zone.height * 0.5];
    }

    let resolved: Vec<(f64, f64)> = zone
        .slots
        .iter()
        .enumerate()
        .map(|(idx, s)| resolve_slot_edges(zone, idx, s))
        .collect();

    let mut gaps = Vec::with_capacity(resolved.len() + 1);
    gaps.push(resolved[0].0);
    for i in 0..resolved.len() - 1 {
        let boundary = (resolved[i].1 + resolved[i + 1].0) * 0.5;
        gaps.push(boundary);
    }
    gaps.push(resolved[resolved.len() - 1].1);
    gaps
}

fn resolve_slot_edges(zone: &DockZone, idx: usize, slot: &DockSlot) -> (f64, f64) {
    if slot.bottom > slot.top {
        return (slot.top, slot.bottom);
    }
    // Legacy mid-only slots: synthesize edges from neighboring mids / zone.
    let mids: Vec<f64> = zone.slots.iter().map(|s| s.mid_y).collect();
    let top = if idx == 0 {
        zone.y
    } else {
        (mids[idx - 1] + mids[idx]) * 0.5
    };
    let bottom = if idx + 1 >= mids.len() {
        zone.y + zone.height
    } else {
        (mids[idx] + mids[idx + 1]) * 0.5
    };
    (top, bottom)
}

fn band_near_gap(band: Rect, gaps: &[f64], half: f64) -> bool {
    gaps.iter()
        .any(|&gap_y| band.y < gap_y + half && band.y + band.height > gap_y - half)
}

fn horizontal_overlap_width(band: Rect, zone: &DockZone) -> f64 {
    let left = band.x.max(zone.x);
    let right = (band.x + band.width).min(zone.x + zone.width);
    (right - left).max(0.0)
}

/// Hit-test a single zone. `prev_inside` applies hysteresis for that zone only.
pub fn update_affinity(
    zone: Option<&DockZone>,
    band: Rect,
    prev_inside: bool,
) -> (bool, bool, usize) {
    let Some(zone) = zone else {
        return (false, false, 0);
    };
    if zone.width <= 0.0 || zone.height <= 0.0 {
        return (false, false, 0);
    }

    let zone_rect = Rect {
        x: zone.x,
        y: zone.y,
        width: zone.width,
        height: zone.height,
    };
    // Horizontal containment only — vertical arming is gap-based.
    let zone_x = Rect {
        x: zone_rect.x,
        y: band.y,
        width: zone_rect.width,
        height: band.height,
    };
    let exit_x = zone_x.inflate(HYSTERESIS_EXIT_PADDING);
    let horiz_ok = if prev_inside {
        band.intersects(exit_x)
    } else {
        band.intersects(zone_x)
    };

    let gaps = insert_gap_ys(zone);
    let gap_half = if prev_inside {
        GAP_HIT_HALF_EXIT
    } else {
        GAP_HIT_HALF_ENTER
    };
    // Empty stack: no insert gaps yet — arm anywhere vertically inside the zone
    // so the first float can redock to a vacant side (left/right edge strip).
    let near_gap = if zone.slots.is_empty() {
        true
    } else {
        band_near_gap(band, &gaps, gap_half)
    };

    let inside = horiz_ok && near_gap;
    let armed = inside;
    let mids: Vec<f64> = zone.slots.iter().map(|s| s.mid_y).collect();
    let insert = compute_insert_index(&mids, band.center_y());
    (inside, armed, insert)
}

/// Hit-test all reported zones; arm at most one (hysteresis prefers current side,
/// otherwise highest horizontal overlap with the probe band).
///
/// `win` is the floating window outer rect. Empty zones use a side-aligned titlebar
/// probe so wide floats can magnet to a thin vacant edge strip.
pub fn update_affinity_multi(
    zones: &HashMap<SidebarSide, DockZone>,
    win: Rect,
    prev_inside: bool,
    prev_side: Option<SidebarSide>,
) -> (bool, bool, usize, Option<SidebarSide>) {
    if zones.is_empty() {
        return (false, false, 0, None);
    }

    struct Hit {
        side: SidebarSide,
        inside: bool,
        armed: bool,
        insert: usize,
        overlap: f64,
    }

    let mut hits: Vec<Hit> = Vec::new();
    for (side, zone) in zones {
        let band = probe_band_for_zone(win, zone);
        let was_this = prev_inside && prev_side == Some(*side);
        let (inside, armed, insert) = update_affinity(Some(zone), band, was_this);
        if inside || armed {
            hits.push(Hit {
                side: *side,
                inside,
                armed,
                insert,
                overlap: horizontal_overlap_width(band, zone),
            });
        }
    }

    if hits.is_empty() {
        return (false, false, 0, None);
    }

    // Hysteresis: stay on the previously armed side while it still hits.
    if let Some(prev) = prev_side {
        if let Some(hit) = hits.iter().find(|h| h.side == prev && h.armed) {
            return (hit.inside, hit.armed, hit.insert, Some(hit.side));
        }
    }

    // Prefer higher overlap; break ties toward the right side (stable).
    hits.sort_by(|a, b| {
        b.overlap
            .partial_cmp(&a.overlap)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| match (a.side, b.side) {
                (SidebarSide::Right, SidebarSide::Left) => std::cmp::Ordering::Less,
                (SidebarSide::Left, SidebarSide::Right) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    });
    let best = &hits[0];
    (best.inside, best.armed, best.insert, Some(best.side))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(
        side: SidebarSide,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        slots: Vec<(f64, f64, f64)>,
    ) -> DockZone {
        DockZone {
            x,
            y,
            width: w,
            height: h,
            scale_factor: 1.0,
            side,
            slots: slots
                .into_iter()
                .map(|(mid_y, top, bottom)| DockSlot {
                    mid_y,
                    top,
                    bottom,
                })
                .collect(),
        }
    }

    fn zone_mids(side: SidebarSide, x: f64, y: f64, w: f64, h: f64, mids: Vec<f64>) -> DockZone {
        DockZone {
            x,
            y,
            width: w,
            height: h,
            scale_factor: 1.0,
            side,
            slots: mids
                .into_iter()
                .map(|mid_y| DockSlot {
                    mid_y,
                    top: 0.0,
                    bottom: 0.0,
                })
                .collect(),
        }
    }

    fn right_zone(x: f64, y: f64, w: f64, h: f64, slots: Vec<(f64, f64, f64)>) -> DockZone {
        zone(SidebarSide::Right, x, y, w, h, slots)
    }

    #[test]
    fn titlebar_band_clips_to_window_height_and_centers_width() {
        let band = titlebar_band(Rect {
            x: 0.0,
            y: 10.0,
            width: 100.0,
            height: 20.0,
        });
        assert_eq!(band.height, 20.0);
        assert_eq!(band.y, 10.0);
        assert_eq!(band.width, HIT_BAND_WIDTH);
        assert_eq!(band.x, (100.0 - HIT_BAND_WIDTH) * 0.5);
    }

    #[test]
    fn titlebar_band_shrinks_to_narrow_window() {
        let band = titlebar_band(Rect {
            x: 50.0,
            y: 0.0,
            width: 40.0,
            height: 200.0,
        });
        assert_eq!(band.width, 40.0);
        assert_eq!(band.x, 50.0);
        assert_eq!(band.height, HIT_BAND_HEIGHT);
    }

    #[test]
    fn null_zone_disarms() {
        let band = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 32.0,
        };
        let (inside, armed, insert) = update_affinity(None, band, true);
        assert!(!inside);
        assert!(!armed);
        assert_eq!(insert, 0);
    }

    #[test]
    fn insert_gaps_include_edges_and_boundaries() {
        let z = right_zone(
            200.0,
            0.0,
            100.0,
            400.0,
            vec![(100.0, 0.0, 200.0), (300.0, 200.0, 400.0)],
        );
        assert_eq!(insert_gap_ys(&z), vec![0.0, 200.0, 400.0]);
    }

    #[test]
    fn middle_of_panel_does_not_arm() {
        let z = right_zone(
            200.0,
            0.0,
            100.0,
            400.0,
            vec![(100.0, 0.0, 200.0), (300.0, 200.0, 400.0)],
        );
        // Titlebar over sidebar but centered on a panel body — must not arm.
        let band = Rect {
            x: 220.0,
            y: 84.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _) = update_affinity(Some(&z), band, false);
        assert!(!inside);
        assert!(!armed);
    }

    #[test]
    fn enter_near_gap_and_exit_hysteresis() {
        let z = right_zone(
            200.0,
            0.0,
            100.0,
            400.0,
            vec![(100.0, 0.0, 200.0), (300.0, 200.0, 400.0)],
        );

        // Horizontally outside
        let band_out = Rect {
            x: 100.0,
            y: 184.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _) = update_affinity(Some(&z), band_out, false);
        assert!(!inside);
        assert!(!armed);

        // Over the middle insert gap
        let band_in = Rect {
            x: 220.0,
            y: 184.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, insert) = update_affinity(Some(&z), band_in, false);
        assert!(inside);
        assert!(armed);
        assert_eq!(insert, 1);

        // Drift slightly off the gap but within exit half-height — stay armed
        let band_pad = Rect {
            x: 220.0,
            y: 184.0 + GAP_HIT_HALF_ENTER + 4.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _) = update_affinity(Some(&z), band_pad, true);
        assert!(inside);
        assert!(armed);

        // Far from every gap — disarm
        let band_far = Rect {
            x: 220.0,
            y: 84.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _) = update_affinity(Some(&z), band_far, true);
        assert!(!inside);
        assert!(!armed);
    }

    #[test]
    fn insert_index_edges() {
        assert_eq!(compute_insert_index(&[], 10.0), 0);
        assert_eq!(compute_insert_index(&[50.0], 10.0), 0);
        assert_eq!(compute_insert_index(&[50.0], 50.0), 1);
        assert_eq!(compute_insert_index(&[40.0, 120.0], 80.0), 1);
        assert_eq!(compute_insert_index(&[40.0, 120.0], 200.0), 2);
    }

    #[test]
    fn floating_only_begin_rejected() {
        let mut c = DockAffinityController::new(true);
        assert!(!c.begin("preview"));
        assert!(!c.begin("preferences"));
        assert!(c.begin("colorlab"));
        assert!(c.session.is_some());
    }

    #[test]
    fn disabled_controller_rejects_begin() {
        let mut c = DockAffinityController::new(false);
        assert!(!c.begin("layers"));
    }

    #[test]
    fn on_moved_emits_only_on_edge() {
        let mut c = DockAffinityController::new(true);
        // Single panel: gaps at top (0) and bottom (400).
        c.set_dock_zone(
            SidebarSide::Right,
            Some(right_zone(
                200.0,
                0.0,
                100.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            )),
        );
        assert!(c.begin("effect"));

        let near_top = Rect {
            x: 210.0,
            y: 0.0,
            width: 80.0,
            height: 300.0,
        };
        let e1 = c.on_moved(near_top);
        assert!(e1.as_ref().is_some_and(|e| e.armed));
        assert_eq!(e1.as_ref().unwrap().side, Some(SidebarSide::Right));
        let e2 = c.on_moved(near_top);
        assert!(e2.is_none());

        let outside = Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 300.0,
        };
        let e3 = c.on_moved(outside);
        assert!(e3.as_ref().is_some_and(|e| !e.armed));
        assert!(e3.as_ref().unwrap().side.is_none());
    }

    #[test]
    fn legacy_mid_only_slots_still_resolve_gaps() {
        let z = zone_mids(SidebarSide::Right, 200.0, 0.0, 100.0, 400.0, vec![100.0, 300.0]);
        assert_eq!(insert_gap_ys(&z), vec![0.0, 200.0, 400.0]);
    }

    #[test]
    fn end_session_emits_disarm() {
        let mut c = DockAffinityController::new(true);
        assert!(c.begin("layers"));
        let ev = c.end_session().unwrap();
        assert!(!ev.armed);
        assert!(ev.insert_index.is_none());
        assert!(ev.side.is_none());
        assert!(c.session.is_none());
    }

    #[test]
    fn two_zones_arms_left_when_band_over_left() {
        let mut zones = HashMap::new();
        zones.insert(
            SidebarSide::Left,
            zone(
                SidebarSide::Left,
                0.0,
                0.0,
                100.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            ),
        );
        zones.insert(
            SidebarSide::Right,
            zone(
                SidebarSide::Right,
                900.0,
                0.0,
                100.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            ),
        );

        let band_left = Rect {
            x: 20.0,
            y: 0.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _, side) = update_affinity_multi(&zones, band_left, false, None);
        assert!(inside);
        assert!(armed);
        assert_eq!(side, Some(SidebarSide::Left));

        let band_right = Rect {
            x: 920.0,
            y: 0.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _, side) = update_affinity_multi(&zones, band_right, false, None);
        assert!(inside);
        assert!(armed);
        assert_eq!(side, Some(SidebarSide::Right));
    }

    #[test]
    fn two_zones_hysteresis_prefers_current_side() {
        let mut zones = HashMap::new();
        // Overlapping zones so both can hit the same band.
        zones.insert(
            SidebarSide::Left,
            zone(
                SidebarSide::Left,
                100.0,
                0.0,
                120.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            ),
        );
        zones.insert(
            SidebarSide::Right,
            zone(
                SidebarSide::Right,
                180.0,
                0.0,
                120.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            ),
        );

        let band = Rect {
            x: 190.0,
            y: 0.0,
            width: 40.0,
            height: 32.0,
        };

        // First pick without history — higher overlap wins (right overlaps more here).
        let (_, armed, _, side) = update_affinity_multi(&zones, band, false, None);
        assert!(armed);
        assert_eq!(side, Some(SidebarSide::Right));

        // With left already armed and still hitting, stay on left despite overlap.
        let (_, armed, _, side) =
            update_affinity_multi(&zones, band, true, Some(SidebarSide::Left));
        assert!(armed);
        assert_eq!(side, Some(SidebarSide::Left));
    }

    #[test]
    fn set_dock_zone_per_side_independent() {
        let mut c = DockAffinityController::new(true);
        c.set_dock_zone(
            SidebarSide::Left,
            Some(zone(
                SidebarSide::Left,
                0.0,
                0.0,
                80.0,
                400.0,
                vec![],
            )),
        );
        c.set_dock_zone(
            SidebarSide::Right,
            Some(zone(
                SidebarSide::Right,
                900.0,
                0.0,
                80.0,
                400.0,
                vec![],
            )),
        );
        assert_eq!(c.zones.len(), 2);

        c.set_dock_zone(SidebarSide::Left, None);
        assert_eq!(c.zones.len(), 1);
        assert!(c.zones.contains_key(&SidebarSide::Right));
    }

    #[test]
    fn session_snapshot_includes_side() {
        let mut c = DockAffinityController::new(true);
        c.set_dock_zone(
            SidebarSide::Left,
            Some(zone(
                SidebarSide::Left,
                0.0,
                0.0,
                100.0,
                400.0,
                vec![(200.0, 0.0, 400.0)],
            )),
        );
        assert!(c.begin("layers"));
        let _ = c.on_moved(Rect {
            x: 10.0,
            y: 0.0,
            width: 80.0,
            height: 300.0,
        });
        let snap = c.session_snapshot().unwrap();
        assert_eq!(snap.0, "layers");
        assert!(snap.1);
        assert_eq!(snap.3, Some(SidebarSide::Left));
    }

    #[test]
    fn empty_zone_arms_full_vertical_extent() {
        let mut zones = HashMap::new();
        // No slots — vacant left edge strip.
        zones.insert(
            SidebarSide::Left,
            DockZone {
                side: SidebarSide::Left,
                x: 0.0,
                y: 40.0,
                width: 40.0,
                height: 700.0,
                scale_factor: 1.0,
                slots: vec![],
            },
        );

        // Titlebar near the TOP of the zone (would miss mid-only gap of ±14px).
        let win_top = Rect {
            x: 0.0,
            y: 50.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, insert, side) =
            update_affinity_multi(&zones, win_top, false, None);
        assert!(inside);
        assert!(armed);
        assert_eq!(insert, 0);
        assert_eq!(side, Some(SidebarSide::Left));

        // And near the bottom.
        let win_bot = Rect {
            x: 0.0,
            y: 680.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _, side) = update_affinity_multi(&zones, win_bot, false, None);
        assert!(inside);
        assert!(armed);
        assert_eq!(side, Some(SidebarSide::Left));
    }

    #[test]
    fn empty_zone_arms_wide_float_via_side_edge_probe() {
        let mut zones = HashMap::new();
        zones.insert(
            SidebarSide::Left,
            DockZone {
                side: SidebarSide::Left,
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 800.0,
                scale_factor: 1.0,
                slots: vec![],
            },
        );

        // Wide Layers-like float flush to the left edge: centered titlebar band
        // sits around x≈143 and would miss a 40px strip; side probe must arm.
        let win = Rect {
            x: 0.0,
            y: 120.0,
            width: 350.0,
            height: 400.0,
        };
        let center = titlebar_band(win);
        assert!(center.x > 40.0, "precondition: center band misses empty strip");

        let (inside, armed, insert, side) = update_affinity_multi(&zones, win, false, None);
        assert!(inside);
        assert!(armed);
        assert_eq!(insert, 0);
        assert_eq!(side, Some(SidebarSide::Left));
    }

    #[test]
    fn empty_zones_disarm() {
        let zones = HashMap::new();
        let band = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 32.0,
        };
        let (inside, armed, _, side) = update_affinity_multi(&zones, band, true, Some(SidebarSide::Right));
        assert!(!inside);
        assert!(!armed);
        assert!(side.is_none());
    }
}
