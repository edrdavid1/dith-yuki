Spec: Fixing the FlexLayout Docking Risks
Project: Dither Yuki (Tauri 2 + Rust + React), flexlayout-react 0.7.15 Basis: FlexLayout: docking integration (as-built B3 + B4a/b/c + B5) Date: 2026-11-11
0. How to use this spec
The spec is split into 10 work items (W1 to W10) and cross-cutting requirements. Each item contains: the problem, the required behavior, the implementation, and acceptance criteria. An item is closed only when all of its acceptance criteria are met and an automated test exists where one is specified.
Rules for the implementer (human or agent):
1. One item, one PR. Do not mix items and do not refactor unrelated code.
2. Do not change the public useLayoutContext() API without an explicit entry in the "Deviations" section of the PR.
3. If a requirement conflicts with the code or the document, do not guess: stop and describe the conflict.
4. Do not bring back global_mouseup, OS startDragging for Flex popouts, or Model cloning.
5. Every new catch must log the cause. Silent fallbacks are forbidden.
6. After each item, update docs/ (the as-built section and "Known issues").
Order: phase 1 (W1, W2) → phase 2 (W3, W4, W5, W7) → phase 3 (W6, W8, W9, W10).
W1. Popout drag must not get stuck (P0)
**Status (2026-11-11):** Implemented — `flexPopoutDragMachine` + pointer-capture `useFlexPopoutJsDrag`; Rust `complete`/`cancel` label-idempotent. Manual OS matrix still required for closeout.
Problem. The window is moved by JS setPosition with IPC latency, and completion is caught only by mouseup inside the WebView. If the window lags and the cursor ends up outside it, mouseup never arrives, the drag gets stuck, and the drop zone stays armed.
Required behavior.
• useFlexPopoutJsDrag is implemented as an explicit state machine: idle → dragging → (completing | cancelled) → idle. Transitions go only through a single idempotent finish(reason) function.
• At drag start: setPointerCapture on the titlebar element (pointer events instead of mouse events).
• Terminating events (any of them calls finish):
    ◦ pointerup → completing (redock if a zone is armed);
    ◦ pointercancel, lostpointercapture, window blur, visibilitychange (hidden), Escape → cancelled;
    ◦ on every pointermove: if event.buttons === 0, treat it as pointerup (the button was released outside the window);
    ◦ watchdog: no pointermove for 1500 ms → check the button; if the state cannot be confirmed, cancelled. Hard cap on drag duration: 60 s.
• Window position is computed as startWindowPos + (cursorScreen - startCursorScreen), not from coordinates relative to the window, so window lag does not accumulate error.
• finish always: releases capture and all listeners (including those on the popout ownerDocument), emits a disarm dock-affinity, clears the drop-edge, drains the setPosition queue (the last cursor position is applied once), and logs the reason.
• Rust: complete_float_drag and cancel_float_drag are idempotent and take a label. A repeated call is a no-op.
Acceptance criteria.
• Unit tests (vitest, fake timers) for the state machine: every terminator leads to idle; a double finish is harmless; buttons===0 ends the drag.
• Manual scenario: a fast drag with release outside the window on macOS, Windows and Linux. The drop-edge always clears and the window does not move after release.
• No listener remains after finish (test on the addEventListener/removeEventListener counter).
W2. Floating tabs after restart and loss of access to a panel (P0)
**Status (2026-11-11):** Implemented — `dockAllFloating` on load; `flex-popout-destroyed` → dock; `FloatingHostStrip` + Window → Return All; preview Return button. Manual: kill-app-with-floats + float-host return still required for closeout.
Problem. The floating state is saved in the model, but popout windows are not recreated at startup. The "Dock back" placeholder was removed, a column with only floating tabs goes into an off-screen host, and there is no way to bring the panel back.
Decision (fixed). When the model is loaded, all floating tabs are force-docked back to the side they were on. Windows are not reopened.
Required behavior.
1. In LayoutContext, after Model.fromJson and applyAppChromePolicy, call dockAllFloating(model): Actions.unFloatTab for every floating tab. It runs before the first render of Layout, with no toast on success, and with a log.
2. Runtime: if a popout window is closed externally (close-poll, OS, WebView crash), the tab is docked automatically to its own side. Source of truth for closure: the Tauri WindowEvent::Destroyed for flex-popout-* plus the existing close-poll.
3. There must always be a return point. Add a permanent UI element that does not depend on a docked sibling:
    ◦ in SidebarCollapseStrip/float-host: a list of icons of this side's floating panels; clicking an icon focuses the popout, and double-click or a "Return" button docks it;
    ◦ a Window menu item: "Return all panels to dock" (calls dockAllFloating and redockAllToDefaultSide).
4. Invariant: each of the 4 panels exists exactly once in Model L/C/R (verified in W7, only invoked here).
Acceptance criteria.
• Test: JSON with three floating tabs gives 0 floating and 3 docked after loading.
• Manual: float all panels, kill the app, launch. All panels are in place.
• Manual: a float-host with a single floating tab always contains a visible return button.
W3. Dependency on a patch in node_modules (P1)
**Status (2026-11-11):** Implemented — `patch-package` + `patches/flexlayout-react+0.7.15.patch`; `__FLEX_PATCH__` marker; `verify:flexlayout-patch` on predev/prebuild/pretest + CI; Vite plugin clears stale prebundle only when verify says so; DEV overlay / PROD float disable via `flexlayoutPatchGate`. Deliberate pin of 0.7.15 recorded in FLEXLAYOUT_DOCKING.md.
Problem. The patch is applied through postinstall. npm ci --ignore-scripts, a CI cache, or a package manager change silently roll back to stock behavior (gray window, shared window name).
Required behavior.
1. Replace the homegrown patch application with patch-package (or pnpm patch if the project uses pnpm): the .patch file lives in the repository (patches/flexlayout-react+0.7.15.patch), and the flexlayout-react version in package.json is pinned exactly ("0.7.15", no ^).
2. The patch adds an export marker to the bundle: __FLEX_PATCH__ = { version: N, features: [...] }.
3. The verify:flexlayout-patch script checks for the marker in node_modules and in the Vite prebundle. It runs in predev, prebuild, pretest, and as a separate CI step.
4. Runtime check at startup: if the marker is missing, show a fatal overlay in dev; in prod, disable float (tabEnableFloat stays false, hide the undock menu) and log the error.
5. The Vite cache reset stays in the ensure-flexlayout-float-patch plugin, but the decision about it is made by verify, not by a blind run.
6. Contract tests for the patch (jsdom/Playwright): the window name has no #; a second float does not reuse the first window; beforeunload during occlusion does not close the popout.
7. Record an explicit decision in the docs: "deliberate pin of 0.7.15". The bump to 0.10.x goes into a separate spike card (see W10).
Acceptance criteria.
• A clean clone plus npm ci --ignore-scripts plus npm run verify:flexlayout-patch fails with a clear error; without --ignore-scripts it passes.
• CI contains the verify step; the build does not pass without it.
W4. Single owner of the Model, no cloning (P1)
**Status (2026-11-11):** Implemented — module-level `layoutStore` + `useSyncExternalStore`; HMR `import.meta.hot.dispose/accept` preserves models; `closeAllFlexPopouts` before load/replace/reset; dock-request listener refcount + generation guard; `noModelFromJsonOutsideStore` + StrictMode tests. Manual: HMR with an open popout still required for closeout.
Problem. Popouts depend on the identity of the mutable Model; layoutEpoch is only a workaround. Hot reload, StrictMode, a repeated provider mount, or a reset to defaults can desynchronize windows and the model.
Required behavior.
1. A LayoutStore outside React state creates and holds the models (a module-level singleton with import.meta.hot.dispose/accept that preserves instances across HMR). React subscribes through useSyncExternalStore (layoutEpoch becomes the store version).
2. All mutations go only through store methods (float, dock, move, redock, reset, load), which call model.doAction and then bump the epoch. Direct new Model/Model.fromJson outside the store is forbidden (ESLint no-restricted-syntax plus a comment).
3. StrictMode: initialization effects and subscriptions are idempotent; a double mount does not create a second flex-panel-dock-request listener (subscription with cleanup and a token guard).
4. reset/load of a new model is allowed only as a sequence: closeAllFlexPopouts() → wait for closure → replace the model → bump the epoch.
5. Main window reload: Rust closes all flex-popout-* on Destroyed/navigation of the main window and, at startup, closes orphaned flex-popout-* (see W5).
Acceptance criteria.
• Test: a double mount of LayoutProvider under StrictMode gives one store and one listener.
• Manual: HMR while a popout is open does not kill the window and does not break the model.
• Code search: zero calls to new Model/Model.fromJson outside the store.
W5. Popout lifecycle and cross-WebView behavior (P1)
**Status (2026-11-11):** Implemented — Rust `flex_popout_lifecycle` closes all `flex-popout-*` on main `CloseRequested`/`Destroyed`/page-load `Started` + startup scrub; `useOwnerDocument` + popout listener guard test; `flexPopoutStyleSync` MutationObserver for head/theme into registered popouts. DPI/scale (item 4) deferred to W6. Manual OS matrix (float/move/close/redock/HMR/theme) still required for closeout.
Problem. A popout is a portal in the same React tree. Reloading the main window or a crash kills all popouts. Events in a foreign document behave differently in WKWebView, WebView2 and WebKitGTK. Styles are cloned by hand.
Required behavior.
1. Rust: on close, reload or crash of the main window, close all windows with the flex-popout- prefix. At application start, close any flex-popout-* found without a main window.
2. All event listeners inside a popout attach to popoutWindow.document/ownerDocument through a useOwnerDocument() helper. Direct window.addEventListener/document.addEventListener in popout components is forbidden (ESLint rule or a code review check).
3. Style sync: a MutationObserver on the main window <head> mirrors added or removed <style>/<link> into open popouts (HMR and theme changes). The theme (prefers-color-scheme, CSS variables) also applies to popouts.
4. Sizes and scale factor: a popout opens correctly when moved to a monitor with a different DPI (resize and position through physical pixels, see W6).
5. Manual test matrix: macOS (WKWebView), Windows (WebView2), Linux (WebKitGTK). For each: float, move, close, redock, HMR, theme change.
Acceptance criteria.
• Reloading the main window (Cmd/Ctrl+R) with three open popouts leaves zero flex-popout-* windows.
• A theme change updates the popout's appearance without a restart.
W6. Zone hit-testing and the label registry (P2)
**Status (2026-11-11):** Implemented — physical-pixel zones (`dockZoneCoords` + Rust scale-aware hit bands); `DockZone.seq` latest-wins; FE once-per-frame reporter; registry SoT (no any/first close fallback); DEV `checkFlexPopoutInvariants`. Manual HiDPI / multi-monitor still required for closeout.
Problem. Asynchronous IPC plus HiDPI/scale factor plus negative monitor coordinates cause zone drift. A desynchronized panel/DOM → flex-popout-* registry leads to closing or docking the wrong window.
Required behavior.
1. A single coordinate system: physical screen pixels. Rust computes zones from the main window's outer_position/inner_position and scale factor; the frontend passes column CSS rectangles converted through the main window's devicePixelRatio. The conversion lives in one function with unit tests.
2. update_dock_zone accepts a monotonic seq; Rust drops packets with a stale seq (latest-wins on the Rust side too). The frontend sends at most once per frame.
3. flexPopoutRegistry: registered when a window is created, removed on Destroyed, one source of truth. If a label does not resolve, the operation (complete, close) is cancelled with a log and a toast, with no fallback to "any/first window".
4. Dev invariant checks: every label in the registry corresponds to a live Tauri window; every floating tab in the model has a registry entry.
5. The disarm dock-affinity is emitted on any completion (tied to W1).
Acceptance criteria.
• Rust tests for zone calculation: negative coordinates, scale 1.0/1.5/2.0, a monitor to the left of the primary one.
• Registry test: closing a window removes its entry; a nonexistent label does not close other windows.
W7. Persistence: version, atomicity, recovery (P2)
**Status (2026-11-11):** Implemented — `shell_layout_persistence` (`layout.json` v1 envelope); atomic temp+fsync+rename + `.bak`; recovery primary→bak→legacy migrate→defaults; corrupt quarantine; FE `normalizeLayoutEnvelope` + `layoutStore` `save_shell_layout` (debounce 500ms / immediate on float-dock-move); `flush-layout` / `layout_flushed` on close. Legacy per-side IPC kept registered but banned from product FE (guard test). Manual: kill after panel move still required for closeout.
Problem. There is no version; a parse error silently resets the layout; the left/right/center files are written independently (moving a panel between sides touches two files); the 500 ms debounce can lose changes on close.
Decision (fixed, revises §12.1 of the source document). A single layout.json file with an envelope:
{ "version": 1, "savedAt": "<ISO>", "left": {…}, "center": {…}, "right": {…} }
Each part is a Model.toJson().
Required behavior.
1. Migration: if layout.json is absent, the legacy files flexlayout_{left,right,center}.json are read (their absence is acceptable), assembled into a v1 envelope, and after a successful write the legacy files are renamed to *.migrated.
2. Atomic write in Rust: write to a temporary file in the same directory, fsync, rename; the previous version is kept as layout.json.bak. All writes are serialized by a mutex.
3. Reading: schema and version validation. Recovery order: layout.json → layout.json.bak → defaults. A corrupted file is renamed to layout.json.corrupt-<ts> and the user sees a toast about the reset.
4. Invariant normalizer after loading: each of the panels layers, effect, colorlab, preview appears exactly once across all models. Missing ones are inserted on their default side, duplicates are removed, unknown component ids are removed. preview is always in center.
5. Saving: changes are debounced at 500 ms; float/dock/move/redock are written immediately. A write always writes all three parts in one envelope.
6. Flush on close: on the main window's CloseRequested, Rust emits flush-layout; the frontend writes the current state synchronously (within a 1000 ms timeout) and replies layout-flushed; on reply or timeout the window closes.
7. Remove the unused save_layout/load_layout/flexlayout_state.json from Rust, or mark them deprecated with a test that the UI does not call them.
8. Update the document: single file, version, migration, recovery order.
Acceptance criteria.
• Rust tests: atomicity (an interrupted write does not damage the old file), recovery from .bak, migration from three legacy files, corrupted JSON.
• TS tests for the normalizer: a missing colorlab, a duplicate layers, an unknown component.
• Manual: kill the process right after moving a panel; after launch the panel is on the new side.
W8. Shell geometry invariants (P2)
**Status (2026-11-11):** Implemented — `computeShellWidths` (+ table tests); AppLayout wired; `canAcceptHorizontalDrop` gated on no emergency collapse; `tauri.conf.json` `minWidth: 800` (320+2×240). Manual: resize 1920→min still required for closeout.
Problem. The rules "column ≥ 240", "up to 2 columns per side", "preview ≥ 320" and "no H-scroll" can contradict each other: 2+2 columns require 960 + 320 = 1280 px plus strips and splitters.
Solution.
1. A pure function computeShellWidths({ viewportWidth, prefs, columnsLeft, columnsRight, collapsed }) returns widths and flags; it does not depend on React and is covered by tests.
2. Priorities: (1) preview ≥ 320; (2) each column ≥ a soft minimum of 240; (3) when space is short, columns shrink proportionally down to a hard minimum of 200; (4) if it still does not fit, a side is temporarily collapsed into a 40 px strip (not written to prefs, restored when the window grows). This deliberately refines the earlier "no auto-collapse" rule: it applies only as an emergency measure, and it must be recorded in the document.
3. Minimum main window size in tauri.conf.json (minWidth) = 320 + 2 × 240 + strips/splitters, so the base case (one column per side) always fits.
4. canAcceptHorizontalDrop takes the current window width into account: a second column on a side is accepted only if computeShellWidths returns it without emergency collapse; otherwise the drop zone is highlighted as unavailable.
5. H-scroll at the shell level is impossible by construction (the function's result always fits in the viewport).
Acceptance criteria.
• Table-driven unit tests: 800, 1000, 1100, 1280, 1920 px; combinations 1+1, 2+1, 2+2 columns; collapsed.
• Manual: resize the window from 1920 down to the minimum; preview is never narrower than 320 px and there is no horizontal scroll.
W9. A single gesture controller (P3)
**Status (2026-11-11):** Implemented — `gestureConfig.ts`; `titlebarGestureMachine` + `TitlebarGestureController` (`filterLayoutAction` debug reject); `useTitlebarGestureController` wired in FlexLayoutContainer. Edge-dock/center-band constants documented in config (patch must stay in sync). Manual OS matrix still required for closeout.
Problem. Undock, native tab-drag, edge-dock with extended zones and EXIT_SLACK_PX are heuristics on top of one mousedown. They test poorly and will drift with every change.
Required behavior.
1. All constants (EXIT_SLACK_PX = 64, edge-dock ~40 px, center drop 0.38–0.62, MAX_SIDE_COLUMNS = 2) live in one file, gestureConfig.ts, with comments on their purpose.
2. A single TitlebarGestureController: on pointerdown it chooses a mode (flex-native-drag or undock) and then works as an explicit state machine. cancelFlexLayoutDrag and the rejection of MOVE_NODE for a floating tab in onAction are encapsulated inside the controller, not spread across components.
3. Guard in onAction: MOVE_NODE for an already floating tab is rejected, with a debug-level log.
4. Tests on synthetic pointer events: movement inside a column does not trigger undock; leaving by more than EXIT_SLACK_PX triggers exactly one floatPanel; a mouseup on the edge-dock after undock does not move the live popout.
Acceptance criteria.
• The table-driven tests pass; changing a constant in gestureConfig.ts changes behavior without edits in other files.
W10. Removing legacy and test safety net (P3)
**Status (2026-11-11):** Implemented — docked/float-host only in AppLayout (DockedSidebar/Mixed already gone; docs/CSS/orphans cleaned); deprecated layout IPC unregistered + `flexlayout_persistence` removed; CI typecheck + lint:security; QA matrix refreshed. Manual three-OS run + automated tauri-driver e2e still open. Spike 0.10.x is a separate card.
Required behavior.
1. Delete DockedSidebar and the Mixed / Legacy-only / Floated flex + legacy modes from AppLayout, leaving two modes per side: docked and float-host. Remove the legacy panel filters and dead Rust commands (check that panels.rs contains only what affinity needs).
2. Automated tests:
    ◦ unit: drag state machine (W1), normalizer and migrations (W7), width solver (W8), gesture controller (W9);
    ◦ Rust: persistence, zone calculation, label registry;
    ◦ e2e smoke (Playwright/WebdriverIO + tauri-driver on Linux and Windows; macOS stays manual): float one panel, float two, redock, restart with persistence.
3. CI runs: lint, typecheck, unit, Rust tests, verify:flexlayout-patch, e2e smoke on Linux.
4. A separate spike card (not part of this spec): bump flexlayout-react to 0.10.x and re-run the patch; the result is recorded in the docs regardless of the outcome.
Acceptance criteria.
• AppLayout has no references to legacy modes; the build and tests are green.
• The manual QA matrix from §13 of the source document is updated and has been run on all three OSes.
Cross-cutting requirements
• Logging: a single [flex-dock] tag, debug/warn/error levels; every finish(reason), every label-resolution failure and every layout recovery is logged.
• No regressions: these scenarios must keep working after every item: float one panel, float two, close → dock, drag-redock to left/right, move between sides, restart with persistence, Preview close → canvas.
• Documentation: when all items are done, update the source document: section 7 (single file), section 12 (closed debts, the pin decision), section 13 (new QA matrix).
Definition of Done
1. W1 and W2 are closed and verified manually on all three OSes.
2. All acceptance criteria of W3 to W10 are met and the automated tests in CI are green.
3. The repository has no global_mouseup, no Model cloning, no unversioned layout files and no silent catch.
4. The documentation matches the code.