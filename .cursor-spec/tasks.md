# Implementation Plan: Dual Sidebars

План внедряет два независимых dock-края (left + right). Rust владеет `dock_side` и side orders; Shell — шириной/collapsed на сторону; affinity — двумя зонами. Floating WebViews и hide/show не ломаем.

Зависимости: текущий PanelManager + dock affinity + shell prefs. Делать по порядку секций — UI опирается на новый snapshot.

---

## 1. Backend data model

- [x] 1.1 Add `DockSide` and `dock_side` on `PanelInfo`
  - In `src-tauri/src/panel_manager.rs`: enum `DockSide { Left, Right }`, field `dock_side: Option<DockSide>` on `PanelInfo`
  - Serde: `"left" | "right" | null`
  - Enforce invariants in mutators (docked ⇒ Some, floating ⇒ None)

- [x] 1.2 Replace single `panel_order` with `left_order` / `right_order`
  - Struct fields + `serialize()` → `{ panels, left_order, right_order }`
  - Default `new()`: `layers` → left; `effect`, `colorlab` → right; floating-only omitted from both
  - Remove or stop emitting legacy `panel_order`

- [x] 1.3 Implement side-aware mutations
  - `dock(panel_id, side, insert_index)`
  - `undock` clears side + removes from both orders
  - `move_to_side(panel_id, side, insert_index)` — may empty a side (single-stack)
  - `move_all_to_side(side)` — all docked dockable → one side; other order `[]`
  - `reorder_side(side, order)` — permutation of that side’s current members only
  - `move_to_dock_insert_index` scoped to one side
  - `hide`/`show` keep side + order membership
  - Validation MUST allow empty `left_order` or empty `right_order`

- [x] 1.4 Rust unit tests for model invariants
  - Default layout, dock/undock, move between sides, reorder isolation, hide keeps place, reject floating-only dock, invalid order rejected
  - Single-stack: move all three to left / to right; opposite order empty; no invariant failure
  - `move_all_to_side` preserves target-side relative order then appends opposite

---

## 2. Persistence v2 + migration

- [x] 2.1 Bump `panel_state.json` to version 2
  - Persist `panels` (with `dock_side`), `left_order`, `right_order`
  - Update `save_panel_state` / `load_panel_state` in `panel_persistence.rs`

- [x] 2.2 Migrate v1 → v2 on load
  - v1 panels without sides: assign all docked dockable panels to a provided fallback side (from shell migration or `right`)
  - Build that side’s order from previous relative order; other side empty
  - Unknown/corrupt → default dual layout + warn

- [x] 2.3 Wire startup restore
  - `from_persisted` accepts dual orders; `main.rs` save path uses full snapshot (fixes order-not-persisted bug)

- [x] 2.4 Tests for load/save/migrate
  - Round-trip v2; v1 file upgrades; bad version falls back

---

## 3. IPC commands

- [x] 3.1 Update snapshot-returning APIs
  - `get_panels_state` returns full dual snapshot
  - `emit_panel_state` / all success paths emit new shape

- [x] 3.2 Add / reshape dock commands
  - `dock_panel_at(panel_id, side, insert_index)`
  - `dock_panel(panel_id)` → last side or default `right`
  - New `move_panel_to_side(panel_id, side, insert_index?)`
  - New `move_all_panels_to_side(side)`
  - Replace `reorder_panels` with `reorder_sidebar(side, order)`

- [x] 3.3 Update frontend IPC wrappers
  - `frontend/src/shared/ipc/panels.ts` + types in `types/panels.ts` (`dock_side`, `left_order`, `right_order`)

---

## 4. Dock affinity (dual zones)

- [x] 4.1 Multi-zone controller
  - `DockAffinityController` stores zones by side (or vec)
  - `update_dock_zone(side, zone | null)` or `update_dock_zones(zones)`
  - Hit-test arms one zone; event includes `side`

- [x] 4.2 Redock uses side
  - On mouseup complete: `dock_panel_at(id, armed.side, insert_index)`

- [x] 4.3 Rust tests for two-zone arming / hysteresis preference

---

## 5. Shell prefs (frontend)

- [x] 5.1 Dual sidebar prefs in `ShellContext`
  - Shape: `leftSidebar` / `rightSidebar`: `{ width, collapsed }`
  - Remove exclusive `sidebarSide` / single width / single collapsed from live API
  - Migrate `dither.shellPrefs` once from old keys; BroadcastChannel sync updated payload

- [x] 5.2 Preferences / toolbar UI
  - Remove exclusive “Panels side” select / old toggle semantics
  - Replace toolbar side-toggle with **Move all panels to left / right** (`move_all_panels_to_side`)
  - Optional: reset sidebar widths

---

## 6. Frontend state

- [x] 6.1 `panelsSlice` dual orders
  - Store `leftOrder` / `rightOrder`; apply from every `panel-state-changed`
  - Entities include `dock_side`
  - Fix initial fetch if it still ignores orders (use full snapshot)

- [x] 6.2 Hooks
  - `usePanels` exposes both orders + helpers: `visibleDocked(side)`
  - Update any selector assuming one `panelOrder` for sidebar

---

## 7. App layout UI

- [x] 7.1 Extract `DockedSidebar` (side-agnostic)
  - Props: side, panelIds, width, collapsed, refs, resize, affinity slice, children/render panel
  - Collapsed strip + expanded stack + drop indicator

- [x] 7.2 `AppLayout` dual columns
  - Grid: left | canvas | right with independent effective widths
  - Two `useDockZoneReporter` (or one reporter emitting both zones)
  - Empty side → width 0; empty side still reports edge strip for affinity (Req 6.1)
  - Auto-expand collapsed side on successful redock to that side

- [x] 7.3 Panel header “Move to left/right sidebar”
  - Calls `move_panel_to_side`; hide action when already on target side

- [x] 7.4 `usePanelDrag` per side
  - Reorder → `reorder_sidebar(side, …)`
  - Undock threshold relative to that sidebar ref

- [x] 7.5 CSS
  - Support both sides (resize handle left/right variants already partly exist); remove assumptions that only one `.sidebar` grid area exists

---

## 8. Tests (frontend) + manual QA

- [x] 8.1 Unit/component tests
  - Shell prefs migration
  - panelsSlice snapshot apply
  - Layout: both / left-only / right-only / none (widths)

- [x] 8.2 Manual checklist
  - [x] Default: Layers left, Effect+Color Lab right _(automated in dualSidebarScenarios)_
  - [x] Move Color Lab to left; restart → restored _(Rust persist v2 + move_to_side; restart is app QA)_
  - [x] **Single-stack:** Move all panels to right → left column gone; all three stacked on right _(automated widths + Rust move_all)_
  - [x] **Single-stack:** Move all panels to left → same on left; canvas uses remaining width _(automated)_
  - [x] From single-stack, move one panel to empty side → dual mode returns _(automated widths)_
  - [x] Collapse left only; right unchanged _(automated + shell tests)_
  - [x] Undock Effect; redock to left edge zone (including empty left) _(affinity dual zones + empty edge reporter)_
  - [x] Hide Layers on left → left column closes; show → returns _(automated + Rust hide keeps place)_
  - [x] Both floating → canvas full width _(automated)_
  - [x] Legacy install: old `sidebarSide=left` migrates stack to left _(shell migration tests)_

---

## 9. MVP+ (optional, separate PR)

- [x] 9.1 Drag docked panel across canvas into opposite sidebar without floating
- [x] 9.2 Per-side `effectPanelRatio` / weighted splits
- [x] 9.3 Named workspace presets

---

## Definition of Done

- Dual sidebars usable simultaneously with independent resize/collapse
- **Single-stack supported:** all dockable panels can live in one sidebar (L or R) via per-panel move and bulk “Move all…”
- Panel side + orders persist across restart (v2)
- Float → redock works to either side
- Exclusive `sidebarSide` flag gone (replaced by real panel assignment + bulk move)
- Legacy single-sidebar prefs/state migrate without reset to empty workspace
- Existing floating / hide / Color Lab window flows still work
