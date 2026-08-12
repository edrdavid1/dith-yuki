# Requirements: Dual Sidebars

## Introduction

Сейчас Main Window имеет **один** dock-столбец: все docked-панели живут либо слева, либо справа (`sidebarSide`), переключение меняет сторону для всего стека сразу. Floating-панели уже вынесены в отдельные OS-окна.

**Dual Sidebars** добавляет два независимых dock-края — Left Sidebar и Right Sidebar — одновременно. Пользователь может держать, например, Layers слева, а Effect Settings и Color Lab справа, не уходя во floating. При этом **все dockable панели могут жить в одном sidebar** (только left или только right) — это полноценный single-stack режим, не ошибка. Floating, hide/show, drag-reorder, drag-undock и drag-to-redock (dock affinity) сохраняются; affinity расширяется до двух зон.

## Glossary

- **Dock_Side**: сторона пристыковки панели — `left` | `right`. Имеет смысл только когда панель в Docked_Mode.
- **Left_Sidebar / Right_Sidebar**: независимые вертикальные колонки Main Window слева и справа от Preview Canvas.
- **Sidebar_Stack**: упорядоченный список docked+visible панелей на одном Dock_Side.
- **Shell_Sidebar_Prefs**: фронтенд-предпочтения ширины/collapsed для каждой стороны (не владеет назначением панелей).
- **PanelManager**: Rust SoT для docked/visible/bounds/order/side панелей.
- **Dock_Zone**: геометрическая hit-зона одной Sidebar_Stack для dock affinity (float → redock).
- **Floating_Only_Panel**: панель, которая никогда не входит в sidebar (`preview`, `preferences`).
- **Panel_State_Changed_Event**: полный snapshot состояния панелей во все окна.
- **Migration**: преобразование старого single-sidebar state в dual без потери workspace.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Два независимых dock одновременно | Произвольный docking в preview / top / bottom |
| **Все dockable панели в одном sidebar** (left *или* right) — полноценный режим | Обязательное использование обеих сторон |
| Per-panel `dock_side` + per-side order | Custom tiling window manager |
| Независимая ширина / collapse каждой стороны | Изменение модели floating WebViews |
| Dual dock affinity (две зоны) | Анимированный fly-in между сторонами |
| Persist side + order + shell widths | Workspace presets / named layouts (позже) |
| Миграция с single `sidebarSide` | Перенос Shell prefs в Rust |

---

## Requirements

### Requirement 1: Dual Dock Data Model

**User Story:** As a developer, I want each dockable panel to know which sidebar it belongs to, so that left and right stacks can be rendered independently.

#### Acceptance Criteria

1. THE PanelManager SHALL extend PanelInfo with `dock_side: "left" | "right" | null`, where `null` is required when `docked = false`, and a non-null value is required when `docked = true` for non-Floating_Only panels.
2. THE PanelManager SHALL maintain two ordered lists of dockable panel IDs: `left_order` and `right_order`, that together contain each dockable panel ID (`effect`, `layers`, `colorlab`) exactly once when that panel is docked; floating and Floating_Only panels SHALL NOT appear in either side order. Either list MAY be empty (all docked panels on the other side is valid).
3. WHEN a panel is undocked, THE PanelManager SHALL set `docked = false`, `dock_side = null`, and remove the panel ID from both side orders.
4. WHEN a panel is docked to a side, THE PanelManager SHALL set `docked = true`, `dock_side` to that side, and insert the panel ID into that side’s order at the requested index (default: append).
5. IF a command attempts to assign a Floating_Only_Panel to a Dock_Side, THEN THE PanelManager SHALL reject the command without mutating state.
6. THE Panel_State_Changed_Event payload SHALL include `panels` (with `dock_side`), `left_order`, and `right_order` (legacy single `panel_order` MAY be omitted or derived as concat for one release if needed for soft migration of listeners).

### Requirement 2: Default Layout and Migration

**User Story:** As a user, I want my existing workspace to upgrade cleanly, so that dual sidebars do not reset my panels.

#### Acceptance Criteria

1. WHEN no persisted dual state exists and the previous shell pref was `sidebarSide = "right"`, THE application SHALL migrate all currently docked dockable panels to `dock_side = "right"` preserving relative order.
2. WHEN no persisted dual state exists and the previous shell pref was `sidebarSide = "left"`, THE application SHALL migrate all currently docked dockable panels to `dock_side = "left"` preserving relative order.
3. WHEN the application starts with neither dual nor legacy layout data, THE PanelManager SHALL use the default: `layers` → left; `effect`, `colorlab` → right (in that order on the right).
4. THE panel disk schema version SHALL bump (v1 → v2) and persist `dock_side` plus both side orders; v1 files SHALL load with migration rules above, then save as v2.
5. THE Shell_Sidebar_Prefs SHALL migrate `sidebarWidth` / `sidebarCollapsed` / `sidebarSide` into `{ left: { width, collapsed }, right: { width, collapsed } }`, applying the old width/collapsed to the side that held panels and using defaults for the empty side.
6. IF persisted dual state is corrupt or fails validation, THEN THE PanelManager SHALL fall back to the default dual layout (criterion 3) and log a warning without crashing.

### Requirement 3: Main Window Dual Layout

**User Story:** As a user, I want panels on both sides of the canvas at once, so that I can keep structure and inspectors visible together.

#### Acceptance Criteria

1. THE Main_Window layout SHALL support columns: optional Left_Sidebar | Preview Canvas | optional Right_Sidebar (plus menubar row).
2. WHEN a Sidebar_Stack has zero docked+visible panels, THAT sidebar column SHALL collapse to width 0 (no chrome, no resize handle).
3. WHEN a Sidebar_Stack has one or more docked+visible panels, THAT sidebar SHALL render those panels in side-order, sharing vertical space equally (same split behavior as today’s single sidebar).
4. EACH non-empty sidebar SHALL support independent horizontal resize with the same width clamp as today (min 240, max 600, default 332) and independent collapse to an icon strip (~40px).
5. WHEN the user collapses or expands one sidebar, THE other sidebar’s width and collapsed state SHALL remain unchanged.
6. THE Preview Canvas SHALL occupy all remaining horizontal space between the two sidebars.
7. Floating_Only panels SHALL continue to never render inside either sidebar.

### Requirement 4: Move Panel Between Sides

**User Story:** As a user, I want to move a docked panel from left to right (and back), so that I can arrange my workspace without undocking — including stacking every panel on one side.

#### Acceptance Criteria

1. THE PanelManager SHALL expose a command (e.g. `move_panel_to_side`) accepting `panel_id`, `dock_side`, and optional `insert_index`.
2. WHEN the command succeeds, THE panel SHALL leave its previous side order, join the target side order at `insert_index` (default append), update `dock_side`, and remain `docked = true` / `visible` unchanged.
3. WHEN `insert_index` is out of range for the target stack, THE PanelManager SHALL clamp to `[0, target_len]`.
4. THE UI SHALL provide at least one explicit control to change a docked panel’s side (header menu or context action: “Move to left/right sidebar”).
5. WHEN a panel is moved between sides via drag (optional MVP+: drag across canvas into the other sidebar’s drop zone), THE same PanelManager mutation path SHALL be used as the explicit command.
6. IF the panel is floating or Floating_Only, THEN the move-to-side command SHALL be rejected.
7. THE user SHALL be able to place **all** dockable docked panels (`effect`, `layers`, `colorlab`) into a single Sidebar_Stack (all left or all right); the opposite side’s order SHALL become empty and that column SHALL close (width 0) while remaining available as an affinity edge for later redock/move.
8. THE UI SHALL expose a bulk action (toolbar, menu, or Preferences) **“Move all panels to left”** / **“Move all panels to right”** that moves every currently docked dockable panel onto the chosen side, preserving relative order among panels that were already on that side and appending the others in a stable order (e.g. previous opposite-side order, then any other docked panels).
9. Bulk move SHALL NOT force floating panels to dock; only panels already in Docked_Mode are relocated. Hidden docked panels SHALL move with the bulk action and keep `visible = false`.

### Requirement 5: Reorder Within a Side

**User Story:** As a user, I want to reorder panels inside one sidebar, so that stack order stays local to that side.

#### Acceptance Criteria

1. WHEN the user drag-reorders inside Left_Sidebar, THE PanelManager SHALL update only `left_order` (right unchanged).
2. WHEN the user drag-reorders inside Right_Sidebar, THE PanelManager SHALL update only `right_order`.
3. THE undock gesture (drag past the outer edge of that sidebar’s bounds) SHALL continue to create a floating window via existing `undock_panel_with_size` semantics.
4. Reorder drop indicators SHALL use midpoints of panels within the active Sidebar_Stack only.

### Requirement 6: Dock Affinity with Two Zones

**User Story:** As a user, I want to drag a floating panel to either sidebar and redock there, so that both edges act as magnets.

#### Acceptance Criteria

1. THE Main_Window SHALL report two Dock_Zones (left and right) whenever the corresponding sidebar column exists or is an active drop target; empty sidebars MAY still expose a thin edge zone so the first panel can redock onto an empty side.
2. THE DockAffinityController SHALL hit-test against all reported zones and arm at most one zone at a time (nearest / overlapping with hysteresis).
3. WHEN redock completes over an armed zone, THE PanelManager SHALL dock the panel to that zone’s Dock_Side at the computed `insert_index` among that side’s docked+visible panels.
4. THE `dock-affinity` event SHALL include `side: "left" | "right"` in addition to `panelId`, `armed`, and `insertIndex`.
5. WHEN a collapsed sidebar’s zone is armed and redock succeeds, THAT sidebar SHALL auto-expand (same as today’s single-sidebar behavior).
6. Existing `startDragging()` + Rust session lifecycle SHALL remain the affinity architecture (no WebView↔WebView chat).

### Requirement 7: Shell Preferences UI

**User Story:** As a user, I want preferences that match dual sidebars, so that I am not offered a single “panels side” toggle that no longer makes sense.

#### Acceptance Criteria

1. THE Preferences panel SHALL replace the exclusive “Panels side” control with dual controls: left sidebar width (or reset), right sidebar width (or reset), and/or collapse defaults if exposed.
2. THE toolbar exclusive `sidebarSide` toggle SHALL be replaced by bulk actions **Move all panels to left / right** (Requirement 4.8), which assign panels rather than flipping a global layout flag.
3. Shell prefs persistence key MAY stay `dither.shellPrefs` with a versioned or additive schema; old clients’ exclusive `sidebarSide` SHALL be migrated once (Requirement 2.5).

### Requirement 8: Persistence Completeness

**User Story:** As a user, I want sidebar assignment and order restored after restart, so that my layout survives sessions.

#### Acceptance Criteria

1. THE `panel_state.json` v2 SHALL persist for each panel: id, docked, visible, window_label, saved_bounds, dock_side.
2. THE `panel_state.json` v2 SHALL persist `left_order` and `right_order`.
3. ON successful panel mutations that affect dock/side/order/visibility/bounds, THE application SHALL debounce-save as today.
4. ON startup, THE PanelManager SHALL restore floating windows for panels with `docked = false` using saved bounds, unchanged from current behavior.
5. Shell dual widths/collapsed SHALL restore from `dither.shellPrefs` independently of Rust panel state.

### Requirement 9: Sync and IPC Compatibility

**User Story:** As a developer, I want all windows to see the same dual-dock snapshot, so that floating windows stay consistent.

#### Acceptance Criteria

1. ALL panel mutations SHALL continue to fan out a full Panel_State_Changed_Event snapshot (no differential sync).
2. `get_panels_state` (or a successor) SHALL return a snapshot that includes side orders and `dock_side`, not only `Vec<PanelInfo>` without order.
3. Frontend RTK `panelsSlice` SHALL store `leftOrder` and `rightOrder` (and `dock_side` on entities) and update them from every snapshot event.
4. Existing hide/show/undock/dock commands SHALL remain valid; `dock_panel` without side SHALL dock to a defined default (last `dock_side` if remembered, else `right`).
5. `dock_panel_at` SHALL accept or be replaced by a variant that includes `dock_side` + `insert_index`.

### Requirement 10: Single-Stack and Empty States

**User Story:** As a user, I want a classic single-sidebar workspace when I prefer it, and a sane UI when sides are empty.

#### Acceptance Criteria

1. WHEN both sidebars are empty (all dockable panels floating or hidden), THE Main_Window SHALL show only menubar + canvas (full width).
2. WHEN all docked dockable panels live on one side (single-stack mode), THE layout SHALL match today’s single-sidebar UX on that side (resize, collapse, affinity, equal vertical split among that side’s visible panels) with no residual chrome from the empty side.
3. Single-stack mode SHALL be a **supported first-class layout**, not an error or transitional state: no validation SHALL require both `left_order` and `right_order` to be non-empty.
4. WHEN the last visible docked panel on a side is hidden, undocked, or moved away, THAT side’s column SHALL close (width 0) without affecting the other side.
5. Hide/show of a docked panel SHALL keep its `dock_side` and position in that side’s order so showing it again restores place.
6. FROM single-stack mode, THE user SHALL be able to move or redock any panel to the empty side to re-enter dual-sidebar mode without restarting the app.
