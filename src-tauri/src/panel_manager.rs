use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Fixed panel identifiers.
pub type PanelId = String; // "effect" | "layers" | "colorlab" | "preview" | "preferences"

/// The set of known panel IDs.
const KNOWN_PANELS: &[&str] = &["effect", "layers", "colorlab", "preview", "preferences"];

/// Panels that may appear in a sidebar stack.
const DOCKABLE_PANELS: &[&str] = &["effect", "layers", "colorlab"];

/// Panels that never enter a sidebar (always out of side orders).
const FLOATING_ONLY_PANELS: &[&str] = &["preview", "preferences"];

/// Which sidebar a docked dockable panel belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    Right,
}

impl DockSide {
    pub fn opposite(self) -> Self {
        match self {
            DockSide::Left => DockSide::Right,
            DockSide::Right => DockSide::Left,
        }
    }
}

/// Saved window position and size in screen pixels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Complete state for a single panel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PanelInfo {
    pub id: PanelId,
    pub docked: bool,
    pub visible: bool,
    pub window_label: Option<String>,
    pub saved_bounds: Option<SavedBounds>,
    /// `Some` iff docked and dockable; always `None` for floating / floating-only.
    #[serde(default)]
    pub dock_side: Option<DockSide>,
}

/// Result of an undock operation, providing info to create the window.
#[derive(Debug)]
pub struct UndockResult {
    pub window_label: String,
    pub url: String,
    pub bounds: Option<SavedBounds>,
    pub already_floating: bool,
    /// Side the panel was on before undock (for revert on window-create failure).
    pub previous_dock_side: Option<DockSide>,
}

/// Errors that can occur in panel operations.
#[derive(Debug, thiserror::Error)]
pub enum PanelError {
    #[error("Unknown panel identifier: {0}")]
    UnknownPanel(String),
    #[error("Window creation failed: {0}")]
    WindowCreationFailed(String),
    #[error("Invalid panel order: {0}")]
    InvalidOrder(String),
    #[error("Panel cannot be assigned to a sidebar: {0}")]
    FloatingOnly(String),
    #[error("Panel is not docked: {0}")]
    NotDocked(String),
}

/// Serialized output from PanelManager (dual-sidebar snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedPanelState {
    pub panels: Vec<PanelInfo>,
    pub left_order: Vec<String>,
    pub right_order: Vec<String>,
}

/// The panel manager holding state for all panels.
/// Lives inside AppState as Mutex<PanelManager>.
pub struct PanelManager {
    panels: HashMap<PanelId, PanelInfo>,
    left_order: Vec<String>,
    right_order: Vec<String>,
    /// Last dock side per panel (survives undock; used by `dock_panel` without side).
    last_dock_sides: HashMap<PanelId, DockSide>,
}

fn is_floating_only(id: &str) -> bool {
    FLOATING_ONLY_PANELS.contains(&id)
}

fn is_dockable(id: &str) -> bool {
    DOCKABLE_PANELS.contains(&id)
}

fn default_dock_side_for(id: &str) -> Option<DockSide> {
    match id {
        "layers" => Some(DockSide::Left),
        "effect" | "colorlab" => Some(DockSide::Right),
        _ => None,
    }
}

fn default_panel_info(id: &str) -> PanelInfo {
    let dock_side = default_dock_side_for(id);
    PanelInfo {
        id: id.to_string(),
        docked: true,
        visible: true,
        window_label: None,
        saved_bounds: None,
        dock_side,
    }
}

impl PanelManager {
    /// Initialize with the default dual layout:
    /// `layers` → left; `effect`, `colorlab` → right; floating-only omitted from both.
    pub fn new() -> Self {
        let mut panels = HashMap::new();
        for &id in KNOWN_PANELS {
            panels.insert(id.to_string(), default_panel_info(id));
        }
        Self {
            panels,
            left_order: vec!["layers".to_string()],
            right_order: vec!["effect".to_string(), "colorlab".to_string()],
            last_dock_sides: HashMap::from([
                ("layers".to_string(), DockSide::Left),
                ("effect".to_string(), DockSide::Right),
                ("colorlab".to_string(), DockSide::Right),
            ]),
        }
    }

    /// Initialize from persisted dual-order state.
    ///
    /// Invalid / missing orders fall back to the default dual layout after ensuring
    /// all known panels exist. Callers that migrate v1 should pass constructed orders.
    pub fn from_persisted(
        panels: Vec<PanelInfo>,
        left_order: Option<Vec<String>>,
        right_order: Option<Vec<String>>,
    ) -> Self {
        let mut map = HashMap::new();
        for mut panel in panels {
            if !KNOWN_PANELS.contains(&panel.id.as_str()) {
                continue;
            }
            Self::normalize_panel_info(&mut panel);
            map.insert(panel.id.clone(), panel);
        }
        for &id in KNOWN_PANELS {
            map.entry(id.to_string())
                .or_insert_with(|| default_panel_info(id));
        }

        let left = left_order.unwrap_or_default();
        let right = right_order.unwrap_or_default();

        if Self::is_valid_side_orders(&left, &right, &map) {
            let mut last_dock_sides = HashMap::new();
            for (id, panel) in &map {
                if let Some(side) = panel.dock_side {
                    last_dock_sides.insert(id.clone(), side);
                }
            }
            Self {
                panels: map,
                left_order: left,
                right_order: right,
                last_dock_sides,
            }
        } else {
            // Rebuild from each panel's dock_side when possible; else default layout.
            let mut rebuilt = Self {
                panels: map,
                left_order: Vec::new(),
                right_order: Vec::new(),
                last_dock_sides: HashMap::new(),
            };
            rebuilt.rebuild_orders_from_panel_sides();
            if rebuilt.left_order.is_empty() && rebuilt.right_order.is_empty() {
                // No docked dockable panels had sides — apply defaults for docked ones.
                rebuilt.apply_default_orders_for_docked();
            }
            for (id, panel) in &rebuilt.panels {
                if let Some(side) = panel.dock_side {
                    rebuilt.last_dock_sides.insert(id.clone(), side);
                }
            }
            rebuilt
        }
    }

    /// Enforce dock_side invariants on a single panel (in-place).
    fn normalize_panel_info(panel: &mut PanelInfo) {
        if is_floating_only(&panel.id) {
            panel.dock_side = None;
            return;
        }
        if !panel.docked {
            panel.dock_side = None;
        } else if panel.dock_side.is_none() && is_dockable(&panel.id) {
            panel.dock_side = default_dock_side_for(&panel.id);
        }
    }

    fn rebuild_orders_from_panel_sides(&mut self) {
        self.left_order.clear();
        self.right_order.clear();
        for &id in DOCKABLE_PANELS {
            let Some(panel) = self.panels.get(id) else {
                continue;
            };
            if !panel.docked {
                continue;
            }
            match panel.dock_side {
                Some(DockSide::Left) => self.left_order.push(id.to_string()),
                Some(DockSide::Right) => self.right_order.push(id.to_string()),
                None => {}
            }
        }
    }

    fn apply_default_orders_for_docked(&mut self) {
        self.left_order.clear();
        self.right_order.clear();
        for &id in DOCKABLE_PANELS {
            let Some(panel) = self.panels.get_mut(id) else {
                continue;
            };
            if !panel.docked {
                continue;
            }
            let side = default_dock_side_for(id).unwrap_or(DockSide::Right);
            panel.dock_side = Some(side);
            match side {
                DockSide::Left => self.left_order.push(id.to_string()),
                DockSide::Right => self.right_order.push(id.to_string()),
            }
        }
    }

    /// Validate dual side orders against panel state.
    fn is_valid_side_orders(
        left: &[String],
        right: &[String],
        panels: &HashMap<PanelId, PanelInfo>,
    ) -> bool {
        let mut seen = HashSet::new();
        for id in left.iter().chain(right.iter()) {
            if !is_dockable(id) || !panels.contains_key(id) {
                return false;
            }
            if !seen.insert(id.as_str()) {
                return false; // duplicate across or within sides
            }
            let panel = &panels[id];
            if !panel.docked {
                return false;
            }
            let expected = if left.iter().any(|x| x == id) {
                DockSide::Left
            } else {
                DockSide::Right
            };
            if panel.dock_side != Some(expected) {
                return false;
            }
        }
        // Every docked dockable panel must appear in exactly one order.
        for &id in DOCKABLE_PANELS {
            let Some(panel) = panels.get(id) else {
                continue;
            };
            if panel.docked && !seen.contains(id) {
                return false;
            }
            if !panel.docked && seen.contains(id) {
                return false;
            }
        }
        true
    }

    fn order(&self, side: DockSide) -> &Vec<String> {
        match side {
            DockSide::Left => &self.left_order,
            DockSide::Right => &self.right_order,
        }
    }

    fn order_mut(&mut self, side: DockSide) -> &mut Vec<String> {
        match side {
            DockSide::Left => &mut self.left_order,
            DockSide::Right => &mut self.right_order,
        }
    }

    fn remove_from_orders(&mut self, id: &str) {
        self.left_order.retain(|x| x != id);
        self.right_order.retain(|x| x != id);
    }

    /// Insert `panel_id` into `side` order at a position derived from `insert_index`
    /// among currently docked+visible panels on that side (excluding the panel itself).
    fn place_at_visible_index(&mut self, panel_id: &str, side: DockSide, insert_index: usize) {
        self.remove_from_orders(panel_id);

        let order_snapshot: Vec<String> = self.order(side).clone();
        let docked_visible: Vec<String> = order_snapshot
            .iter()
            .filter(|id| {
                self.panels
                    .get(*id)
                    .map(|p| p.docked && p.visible)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        let abs_pos = if insert_index >= docked_visible.len() {
            match docked_visible.last() {
                Some(last) => {
                    order_snapshot
                        .iter()
                        .position(|x| x == last)
                        .unwrap_or(order_snapshot.len())
                        + 1
                }
                None => order_snapshot.len(),
            }
        } else {
            let target = &docked_visible[insert_index];
            order_snapshot
                .iter()
                .position(|x| x == target)
                .unwrap_or(0)
        };

        let abs_pos = abs_pos.min(order_snapshot.len());
        let mut new_order = order_snapshot;
        new_order.insert(abs_pos, panel_id.to_string());
        *self.order_mut(side) = new_order;

        if let Some(panel) = self.panels.get_mut(panel_id) {
            panel.dock_side = Some(side);
        }
        self.last_dock_sides
            .insert(panel_id.to_string(), side);
    }

    /// Insert into side order by absolute index among all members of that side.
    fn place_at_absolute_index(&mut self, panel_id: &str, side: DockSide, insert_index: usize) {
        self.remove_from_orders(panel_id);
        let len = self.order(side).len();
        let pos = insert_index.min(len);
        self.order_mut(side).insert(pos, panel_id.to_string());
        if let Some(panel) = self.panels.get_mut(panel_id) {
            panel.dock_side = Some(side);
        }
        self.last_dock_sides
            .insert(panel_id.to_string(), side);
    }

    /// Side to use for `dock_panel` without an explicit side: last remembered, else right.
    pub fn remembered_dock_side(&self, id: &str) -> DockSide {
        self.last_dock_sides
            .get(id)
            .copied()
            .or_else(|| self.panels.get(id).and_then(|p| p.dock_side))
            .unwrap_or(DockSide::Right)
    }

    /// Validate that a panel ID is in the known set.
    pub fn validate_panel_id(&self, id: &str) -> Result<(), PanelError> {
        if self.panels.contains_key(id) {
            Ok(())
        } else {
            Err(PanelError::UnknownPanel(id.to_string()))
        }
    }

    fn ensure_dockable(&self, id: &str) -> Result<(), PanelError> {
        self.validate_panel_id(id)?;
        if is_floating_only(id) {
            return Err(PanelError::FloatingOnly(id.to_string()));
        }
        if !is_dockable(id) {
            return Err(PanelError::FloatingOnly(id.to_string()));
        }
        Ok(())
    }

    /// Get a snapshot of all panel states (stable known-panel order).
    pub fn get_state(&self) -> Vec<PanelInfo> {
        KNOWN_PANELS
            .iter()
            .filter_map(|&id| self.panels.get(id).cloned())
            .collect()
    }

    pub fn get_left_order(&self) -> &[String] {
        &self.left_order
    }

    pub fn get_right_order(&self) -> &[String] {
        &self.right_order
    }

    /// Get state snapshot including both side orders.
    pub fn get_state_with_orders(&self) -> (Vec<PanelInfo>, Vec<String>, Vec<String>) {
        (
            self.get_state(),
            self.left_order.clone(),
            self.right_order.clone(),
        )
    }

    /// Undock a panel: sets docked=false, clears dock_side, removes from both orders.
    pub fn undock(&mut self, id: &str) -> Result<UndockResult, PanelError> {
        self.validate_panel_id(id)?;

        let previous_dock_side = self.panels.get(id).unwrap().dock_side;
        if let Some(side) = previous_dock_side {
            self.last_dock_sides.insert(id.to_string(), side);
        }
        let already_floating = !self.panels.get(id).unwrap().docked;

        // If already floating, return result indicating no new window needed.
        if already_floating {
            let panel = self.panels.get(id).unwrap();
            return Ok(UndockResult {
                window_label: panel
                    .window_label
                    .clone()
                    .unwrap_or_else(|| format!("panel-{}", id)),
                url: format!("index.html?panel={}", id),
                bounds: panel.saved_bounds.clone(),
                already_floating: true,
                previous_dock_side,
            });
        }

        let window_label = format!("panel-{}", id);
        let bounds = {
            let panel = self.panels.get_mut(id).unwrap();
            panel.docked = false;
            panel.dock_side = None;
            panel.window_label = Some(window_label.clone());
            panel.saved_bounds.clone()
        };
        self.remove_from_orders(id);

        Ok(UndockResult {
            window_label,
            url: format!("index.html?panel={}", id),
            bounds,
            already_floating: false,
            previous_dock_side,
        })
    }

    /// Dock a panel back to its host.
    ///
    /// Dockable panels: `docked = true`, `dock_side = Some(side)`, insert into that side's order.
    /// Floating-only (`preview`, `preferences`): return to main-window host (`docked = true`,
    /// no sidebar order) — preview reappears in the canvas; preferences just closes.
    /// Returns the window label to close (if any).
    pub fn dock(
        &mut self,
        id: &str,
        side: DockSide,
        insert_index: usize,
    ) -> Result<Option<String>, PanelError> {
        self.validate_panel_id(id)?;

        // Floating-only never joins a sidebar; "dock" = return to main host.
        if is_floating_only(id) {
            if self.panels.get(id).unwrap().docked {
                return Ok(None);
            }
            let old_label = {
                let panel = self.panels.get_mut(id).unwrap();
                let old_label = panel.window_label.take();
                panel.docked = true;
                panel.dock_side = None;
                old_label
            };
            return Ok(old_label);
        }

        self.ensure_dockable(id)?;

        if self.panels.get(id).unwrap().docked {
            // Already docked — no-op (use move_to_side / move_to_dock_insert_index to relocate).
            return Ok(None);
        }

        let old_label = {
            let panel = self.panels.get_mut(id).unwrap();
            let old_label = panel.window_label.take();
            panel.docked = true;
            panel.window_label = None;
            old_label
        };

        self.place_at_visible_index(id, side, insert_index);
        Ok(old_label)
    }

    /// Move a docked panel to another side (or same side) at an absolute insert index.
    /// May empty a side (single-stack). Rejects floating / floating-only panels.
    pub fn move_to_side(
        &mut self,
        id: &str,
        side: DockSide,
        insert_index: usize,
    ) -> Result<(), PanelError> {
        self.ensure_dockable(id)?;

        let panel = self.panels.get(id).unwrap();
        if !panel.docked {
            return Err(PanelError::NotDocked(id.to_string()));
        }

        self.place_at_absolute_index(id, side, insert_index);
        Ok(())
    }

    /// Move all currently docked dockable panels onto `side`.
    /// Preserves target-side relative order, then appends the opposite side's order.
    /// Floating panels are untouched. Either resulting opposite order is empty.
    pub fn move_all_to_side(&mut self, side: DockSide) -> Result<(), PanelError> {
        let (keep, append) = match side {
            DockSide::Left => (self.left_order.clone(), self.right_order.clone()),
            DockSide::Right => (self.right_order.clone(), self.left_order.clone()),
        };
        let new_order: Vec<String> = keep.into_iter().chain(append).collect();

        self.left_order.clear();
        self.right_order.clear();
        *self.order_mut(side) = new_order.clone();

        for id in &new_order {
            if let Some(panel) = self.panels.get_mut(id) {
                panel.dock_side = Some(side);
            }
            self.last_dock_sides.insert(id.clone(), side);
        }
        Ok(())
    }

    /// Swap left and right docked stacks (orders + dock_side). Floating panels stay put.
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.left_order, &mut self.right_order);
        for id in &self.left_order {
            if let Some(panel) = self.panels.get_mut(id) {
                panel.dock_side = Some(DockSide::Left);
            }
            self.last_dock_sides.insert(id.clone(), DockSide::Left);
        }
        for id in &self.right_order {
            if let Some(panel) = self.panels.get_mut(id) {
                panel.dock_side = Some(DockSide::Right);
            }
            self.last_dock_sides.insert(id.clone(), DockSide::Right);
        }
    }

    /// Reorder panels on one side. `order` must be a permutation of that side's
    /// current members only. Empty order is valid when the side is already empty.
    pub fn reorder_side(&mut self, side: DockSide, order: Vec<String>) -> Result<(), PanelError> {
        let current = self.order(side).clone();
        if order.len() != current.len() {
            return Err(PanelError::InvalidOrder(format!(
                "expected {} panels on {:?}, got {}",
                current.len(),
                side,
                order.len()
            )));
        }

        let current_set: HashSet<&str> = current.iter().map(|s| s.as_str()).collect();
        let mut seen = HashSet::new();
        for id in &order {
            if !current_set.contains(id.as_str()) {
                return Err(PanelError::InvalidOrder(format!(
                    "panel {} is not on {:?}",
                    id, side
                )));
            }
            if !seen.insert(id.as_str()) {
                return Err(PanelError::InvalidOrder(format!(
                    "duplicate panel ID: {}",
                    id
                )));
            }
        }

        *self.order_mut(side) = order;
        Ok(())
    }

    /// Hide a panel: sets visible=false. Keeps dock_side and order membership.
    pub fn hide(&mut self, id: &str) -> Result<bool, PanelError> {
        self.validate_panel_id(id)?;

        let panel = self.panels.get_mut(id).unwrap();
        if !panel.visible {
            return Ok(false);
        }
        panel.visible = false;
        Ok(true)
    }

    /// Show a panel: sets visible=true. Keeps dock_side and order membership.
    pub fn show(&mut self, id: &str) -> Result<bool, PanelError> {
        self.validate_panel_id(id)?;

        let panel = self.panels.get_mut(id).unwrap();
        if panel.visible {
            return Ok(false);
        }
        panel.visible = true;
        Ok(true)
    }

    /// Update saved bounds for a panel (called on window move/resize).
    pub fn update_bounds(&mut self, id: &str, bounds: SavedBounds) -> Result<(), PanelError> {
        self.validate_panel_id(id)?;
        let panel = self.panels.get_mut(id).unwrap();
        panel.saved_bounds = Some(bounds);
        Ok(())
    }

    /// Serialize panel state for disk / event payloads.
    pub fn serialize(&self) -> SerializedPanelState {
        SerializedPanelState {
            panels: self.get_state(),
            left_order: self.left_order.clone(),
            right_order: self.right_order.clone(),
        }
    }

    /// Move an already-docked panel to `insert_index` among docked+visible panels on `side`.
    /// Updates `dock_side` and side orders; does not change the `docked` flag.
    /// For float → redock, use [`Self::dock`] instead (atomic).
    pub fn move_to_dock_insert_index(
        &mut self,
        panel_id: &str,
        side: DockSide,
        insert_index: usize,
    ) -> Result<(), PanelError> {
        self.ensure_dockable(panel_id)?;
        if !self.panels.get(panel_id).unwrap().docked {
            return Err(PanelError::NotDocked(panel_id.to_string()));
        }
        self.place_at_visible_index(panel_id, side, insert_index);
        Ok(())
    }

    /// Debug/test helper: assert model invariants hold.
    #[cfg(test)]
    fn assert_invariants(&self) {
        assert!(
            Self::is_valid_side_orders(&self.left_order, &self.right_order, &self.panels),
            "side order invariants violated: left={:?} right={:?} panels={:?}",
            self.left_order,
            self.right_order,
            self.get_state()
        );
        for panel in self.panels.values() {
            if is_floating_only(&panel.id) {
                assert!(panel.dock_side.is_none());
                assert!(!self.left_order.contains(&panel.id));
                assert!(!self.right_order.contains(&panel.id));
            } else if !panel.docked {
                assert!(panel.dock_side.is_none());
            } else if is_dockable(&panel.id) {
                assert!(panel.dock_side.is_some());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_layers_left_inspectors_right() {
        let pm = PanelManager::new();
        pm.assert_invariants();
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["effect", "colorlab"]);

        let layers = pm.panels.get("layers").unwrap();
        assert_eq!(layers.dock_side, Some(DockSide::Left));
        assert!(layers.docked);

        let effect = pm.panels.get("effect").unwrap();
        assert_eq!(effect.dock_side, Some(DockSide::Right));

        let preview = pm.panels.get("preview").unwrap();
        assert!(preview.dock_side.is_none());
        let preferences = pm.panels.get("preferences").unwrap();
        assert!(preferences.dock_side.is_none());
    }

    #[test]
    fn dock_panel_remembers_last_side_after_undock() {
        let mut pm = PanelManager::new();
        assert_eq!(pm.remembered_dock_side("layers"), DockSide::Left);
        pm.undock("layers").unwrap();
        assert_eq!(pm.remembered_dock_side("layers"), DockSide::Left);
        pm.dock("layers", pm.remembered_dock_side("layers"), usize::MAX)
            .unwrap();
        assert_eq!(pm.panels["layers"].dock_side, Some(DockSide::Left));
        assert_eq!(pm.get_left_order(), &["layers"]);
    }

    #[test]
    fn dock_side_serde_left_right_null() {
        let left = DockSide::Left;
        let right = DockSide::Right;
        assert_eq!(serde_json::to_string(&left).unwrap(), "\"left\"");
        assert_eq!(serde_json::to_string(&right).unwrap(), "\"right\"");

        let info = PanelInfo {
            id: "layers".into(),
            docked: false,
            visible: true,
            window_label: None,
            saved_bounds: None,
            dock_side: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"dock_side\":null"));
    }

    #[test]
    fn undock_clears_side_and_removes_from_orders() {
        let mut pm = PanelManager::new();
        let result = pm.undock("layers").unwrap();
        assert!(!result.already_floating);
        assert_eq!(result.previous_dock_side, Some(DockSide::Left));

        let panel = pm.panels.get("layers").unwrap();
        assert!(!panel.docked);
        assert!(panel.dock_side.is_none());
        assert!(!pm.left_order.iter().any(|id| id == "layers"));
        assert!(!pm.right_order.iter().any(|id| id == "layers"));
        pm.assert_invariants();
    }

    #[test]
    fn dock_sets_side_and_inserts() {
        let mut pm = PanelManager::new();
        pm.undock("layers").unwrap();
        let label = pm.dock("layers", DockSide::Right, usize::MAX).unwrap();
        assert_eq!(label, Some("panel-layers".into()));

        let panel = pm.panels.get("layers").unwrap();
        assert!(panel.docked);
        assert_eq!(panel.dock_side, Some(DockSide::Right));
        assert_eq!(pm.get_right_order(), &["effect", "colorlab", "layers"]);
        assert!(pm.get_left_order().is_empty());
        pm.assert_invariants();
    }

    #[test]
    fn dock_returns_floating_only_to_main_without_sidebar() {
        let mut pm = PanelManager::new();
        pm.undock("preview").unwrap();
        let label = pm
            .dock("preview", DockSide::Left, 0)
            .unwrap()
            .expect("window label");
        assert_eq!(label, "panel-preview");
        let preview = pm.panels.get("preview").unwrap();
        assert!(preview.docked);
        assert!(preview.dock_side.is_none());
        assert!(preview.window_label.is_none());
        assert!(!pm.get_left_order().iter().any(|id| id == "preview"));
        assert!(!pm.get_right_order().iter().any(|id| id == "preview"));

        pm.undock("preferences").unwrap();
        pm.dock("preferences", DockSide::Right, 0).unwrap();
        let prefs = pm.panels.get("preferences").unwrap();
        assert!(prefs.docked);
        assert!(prefs.dock_side.is_none());
        pm.assert_invariants();
    }

    #[test]
    fn move_to_side_may_empty_a_side() {
        let mut pm = PanelManager::new();
        pm.move_to_side("effect", DockSide::Left, usize::MAX).unwrap();
        pm.move_to_side("colorlab", DockSide::Left, usize::MAX)
            .unwrap();
        assert_eq!(pm.get_left_order(), &["layers", "effect", "colorlab"]);
        assert!(pm.get_right_order().is_empty());
        pm.assert_invariants();
    }

    #[test]
    fn move_to_side_rejects_floating() {
        let mut pm = PanelManager::new();
        pm.undock("layers").unwrap();
        let err = pm
            .move_to_side("layers", DockSide::Right, 0)
            .unwrap_err();
        assert!(matches!(err, PanelError::NotDocked(_)));
    }

    #[test]
    fn move_all_to_side_preserves_target_then_appends_opposite() {
        let mut pm = PanelManager::new();
        // left: layers; right: effect, colorlab
        pm.move_all_to_side(DockSide::Left).unwrap();
        assert_eq!(pm.get_left_order(), &["layers", "effect", "colorlab"]);
        assert!(pm.get_right_order().is_empty());
        for id in ["layers", "effect", "colorlab"] {
            assert_eq!(pm.panels[id].dock_side, Some(DockSide::Left));
        }
        pm.assert_invariants();

        pm.move_all_to_side(DockSide::Right).unwrap();
        assert!(pm.get_left_order().is_empty());
        assert_eq!(pm.get_right_order(), &["layers", "effect", "colorlab"]);
        pm.assert_invariants();
    }

    #[test]
    fn swap_sides_exchanges_orders_and_dock_sides() {
        let mut pm = PanelManager::new();
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["effect", "colorlab"]);
        pm.swap_sides();
        assert_eq!(pm.get_left_order(), &["effect", "colorlab"]);
        assert_eq!(pm.get_right_order(), &["layers"]);
        assert_eq!(pm.panels["layers"].dock_side, Some(DockSide::Right));
        assert_eq!(pm.panels["effect"].dock_side, Some(DockSide::Left));
        assert_eq!(pm.panels["colorlab"].dock_side, Some(DockSide::Left));
        pm.assert_invariants();
        pm.swap_sides();
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["effect", "colorlab"]);
        pm.assert_invariants();
    }

    #[test]
    fn move_all_to_side_ignores_floating() {
        let mut pm = PanelManager::new();
        pm.undock("colorlab").unwrap();
        pm.move_all_to_side(DockSide::Left).unwrap();
        assert_eq!(pm.get_left_order(), &["layers", "effect"]);
        assert!(pm.get_right_order().is_empty());
        assert!(!pm.panels["colorlab"].docked);
        assert!(pm.panels["colorlab"].dock_side.is_none());
        pm.assert_invariants();
    }

    #[test]
    fn reorder_side_isolates_sides() {
        let mut pm = PanelManager::new();
        pm.reorder_side(
            DockSide::Right,
            vec!["colorlab".into(), "effect".into()],
        )
        .unwrap();
        assert_eq!(pm.get_right_order(), &["colorlab", "effect"]);
        assert_eq!(pm.get_left_order(), &["layers"]);
        pm.assert_invariants();
    }

    #[test]
    fn reorder_side_rejects_invalid_permutation() {
        let mut pm = PanelManager::new();
        // Wrong members (includes left panel)
        let err = pm
            .reorder_side(
                DockSide::Right,
                vec!["layers".into(), "effect".into()],
            )
            .unwrap_err();
        assert!(matches!(err, PanelError::InvalidOrder(_)));

        // Wrong length
        let err = pm
            .reorder_side(DockSide::Right, vec!["effect".into()])
            .unwrap_err();
        assert!(matches!(err, PanelError::InvalidOrder(_)));
    }

    #[test]
    fn hide_keeps_side_and_order() {
        let mut pm = PanelManager::new();
        pm.hide("layers").unwrap();
        assert!(!pm.panels["layers"].visible);
        assert_eq!(pm.panels["layers"].dock_side, Some(DockSide::Left));
        assert_eq!(pm.get_left_order(), &["layers"]);
        pm.show("layers").unwrap();
        assert!(pm.panels["layers"].visible);
        assert_eq!(pm.get_left_order(), &["layers"]);
        pm.assert_invariants();
    }

    #[test]
    fn move_to_dock_insert_index_scoped_to_side() {
        let mut pm = PanelManager::new();
        // Move layers onto right among visible docked (effect, colorlab).
        pm.move_to_dock_insert_index("layers", DockSide::Right, 1)
            .unwrap();
        assert_eq!(pm.get_left_order(), &[] as &[String]);
        let right = pm.get_right_order();
        assert_eq!(right[0], "effect");
        assert_eq!(right[1], "layers");
        assert_eq!(right[2], "colorlab");
        assert_eq!(pm.panels["layers"].dock_side, Some(DockSide::Right));
        pm.assert_invariants();
    }

    #[test]
    fn dock_from_floating_places_on_side() {
        let mut pm = PanelManager::new();
        pm.undock("colorlab").unwrap();
        pm.dock("colorlab", DockSide::Left, 0).unwrap();
        assert_eq!(pm.get_left_order(), &["colorlab", "layers"]);
        assert_eq!(pm.get_right_order(), &["effect"]);
        pm.assert_invariants();
    }

    #[test]
    fn move_to_dock_insert_index_rejects_floating() {
        let mut pm = PanelManager::new();
        pm.undock("layers").unwrap();
        let err = pm
            .move_to_dock_insert_index("layers", DockSide::Right, 0)
            .unwrap_err();
        assert!(matches!(err, PanelError::NotDocked(_)));
    }

    #[test]
    fn single_stack_all_three_left_or_right() {
        let mut pm = PanelManager::new();
        pm.move_all_to_side(DockSide::Left).unwrap();
        assert_eq!(pm.left_order.len(), 3);
        assert!(pm.right_order.is_empty());
        pm.assert_invariants();

        pm.move_all_to_side(DockSide::Right).unwrap();
        assert_eq!(pm.right_order.len(), 3);
        assert!(pm.left_order.is_empty());
        pm.assert_invariants();
    }

    #[test]
    fn empty_side_orders_are_valid() {
        let mut pm = PanelManager::new();
        pm.move_all_to_side(DockSide::Right).unwrap();
        assert!(pm.get_left_order().is_empty());
        assert!(PanelManager::is_valid_side_orders(
            pm.get_left_order(),
            pm.get_right_order(),
            &pm.panels
        ));
    }

    #[test]
    fn serialize_emits_dual_orders_not_panel_order() {
        let pm = PanelManager::new();
        let serialized = pm.serialize();
        assert_eq!(serialized.left_order, vec!["layers"]);
        assert_eq!(serialized.right_order, vec!["effect", "colorlab"]);
        let json = serde_json::to_string(&serialized).unwrap();
        assert!(!json.contains("panel_order"));
        assert!(json.contains("left_order"));
        assert!(json.contains("right_order"));
    }

    #[test]
    fn new_initializes_five_panels() {
        let pm = PanelManager::new();
        let state = pm.get_state();
        assert_eq!(state.len(), 5);
        for panel in &state {
            assert!(panel.docked);
            assert!(panel.visible);
            assert!(panel.window_label.is_none());
            assert!(panel.saved_bounds.is_none());
        }
    }

    #[test]
    fn validate_panel_id_rejects_unknown() {
        let pm = PanelManager::new();
        assert!(pm.validate_panel_id("effect").is_ok());
        assert!(pm.validate_panel_id("unknown").is_err());
    }

    #[test]
    fn undock_already_floating_returns_already_floating() {
        let mut pm = PanelManager::new();
        pm.undock("layers").unwrap();
        let result = pm.undock("layers").unwrap();
        assert!(result.already_floating);
        assert_eq!(result.window_label, "panel-layers");
    }

    #[test]
    fn undock_preserves_saved_bounds() {
        let mut pm = PanelManager::new();
        let bounds = SavedBounds {
            x: 100,
            y: 200,
            width: 400,
            height: 600,
        };
        pm.update_bounds("effect", bounds.clone()).unwrap();
        let result = pm.undock("effect").unwrap();
        assert_eq!(result.bounds, Some(bounds));
    }

    #[test]
    fn dock_preserves_visible_and_saved_bounds() {
        let mut pm = PanelManager::new();
        let bounds = SavedBounds {
            x: 50,
            y: 60,
            width: 300,
            height: 400,
        };
        pm.update_bounds("effect", bounds.clone()).unwrap();
        pm.hide("effect").unwrap();
        pm.undock("effect").unwrap();
        pm.dock("effect", DockSide::Right, usize::MAX).unwrap();

        let panel = pm.panels.get("effect").unwrap();
        assert!(!panel.visible);
        assert_eq!(panel.saved_bounds, Some(bounds));
        assert_eq!(panel.dock_side, Some(DockSide::Right));
    }

    #[test]
    fn hide_already_hidden_returns_false() {
        let mut pm = PanelManager::new();
        pm.hide("effect").unwrap();
        assert!(!pm.hide("effect").unwrap());
    }

    #[test]
    fn show_already_visible_returns_false() {
        let mut pm = PanelManager::new();
        assert!(!pm.show("layers").unwrap());
    }

    #[test]
    fn from_persisted_restores_dual_orders() {
        let panels = vec![
            PanelInfo {
                id: "effect".to_string(),
                docked: false,
                visible: true,
                window_label: Some("panel-effect".to_string()),
                saved_bounds: Some(SavedBounds {
                    x: 100,
                    y: 200,
                    width: 400,
                    height: 600,
                }),
                dock_side: None,
            },
            PanelInfo {
                id: "layers".to_string(),
                docked: true,
                visible: false,
                window_label: None,
                saved_bounds: None,
                dock_side: Some(DockSide::Left),
            },
            PanelInfo {
                id: "colorlab".to_string(),
                docked: true,
                visible: true,
                window_label: None,
                saved_bounds: None,
                dock_side: Some(DockSide::Right),
            },
        ];
        let pm = PanelManager::from_persisted(
            panels,
            Some(vec!["layers".into()]),
            Some(vec!["colorlab".into()]),
        );
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["colorlab"]);
        assert!(!pm.panels["effect"].docked);
        assert!(!pm.panels["layers"].visible);
        pm.assert_invariants();
    }

    #[test]
    fn from_persisted_ignores_unknown_panels() {
        let panels = vec![PanelInfo {
            id: "unknown_panel".to_string(),
            docked: false,
            visible: true,
            window_label: Some("panel-unknown".to_string()),
            saved_bounds: None,
            dock_side: None,
        }];
        let pm = PanelManager::from_persisted(panels, None, None);
        assert_eq!(pm.get_state().len(), 5);
        assert!(pm.get_state().iter().all(|p| p.id != "unknown_panel"));
    }

    #[test]
    fn from_persisted_invalid_orders_fall_back() {
        let pm = PanelManager::from_persisted(
            vec![],
            Some(vec!["effect".into(), "effect".into()]),
            Some(vec![]),
        );
        // Falls back to default dual layout for docked defaults.
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["effect", "colorlab"]);
        pm.assert_invariants();
    }

    #[test]
    fn get_state_returns_stable_order() {
        let pm = PanelManager::new();
        let state = pm.get_state();
        assert_eq!(state[0].id, "effect");
        assert_eq!(state[1].id, "layers");
        assert_eq!(state[2].id, "colorlab");
        assert_eq!(state[3].id, "preview");
        assert_eq!(state[4].id, "preferences");
    }

    #[test]
    fn move_to_side_clamps_insert_index() {
        let mut pm = PanelManager::new();
        pm.move_to_side("layers", DockSide::Right, 999).unwrap();
        assert_eq!(pm.get_right_order(), &["effect", "colorlab", "layers"]);
        assert!(pm.get_left_order().is_empty());
        pm.assert_invariants();
    }
}
