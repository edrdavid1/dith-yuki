# Tauri IPC inventory

Canonical domain `invoke` lives under `frontend/src/shared/ipc/`. Prefer those
wrappers over raw `@tauri-apps/api` `invoke` outside this folder.

## Current modules (`shared/ipc/`)

| Module | Domain |
|---|---|
| `app.ts` | App-level helpers |
| `appIcon.ts` | Custom app icon |
| `ascii.ts` | ASCII export / clipboard / preview mode |
| `dialogs.ts` | Native open/save dialogs |
| `document.ts` | Document snapshot / lifecycle |
| `errors.ts` | Error mapping |
| `events.ts` | `listen` / event names |
| `filters.ts` | Filter stack mutations |
| `layers.ts` | Layer tree |
| `palettes.ts` | Color Lab / palettes |
| `panels.ts` | Panel / FlexLayout state |
| `pattern.ts` / `patternLibrary.ts` | Patterns (`.dyuki`) |
| `project.ts` | Open / save project |
| `recent.ts` | Recent files |
| `recovery.ts` | Crash recovery roster |
| `registry.ts` | Algorithm registry / schemas |
| `selection.ts` | Selection |
| `undo.ts` | Undo / redo |
| `updates.ts` | In-app updater |
| `viewport.ts` | Viewport / pan-zoom |

Compat barrels (re-exports only): `frontend/src/ipc/commands.ts`,
`frontend/src/ipc/panelCommands.ts`.

Window chrome (`getCurrentWindow`) and `lib/platform.ts` may call non-domain
Tauri APIs directly. Raw-invoke allowlist / remaining debt:
[`INVOKE_AUDIT.md`](./INVOKE_AUDIT.md).

## Historical P0 audit (pre-consolidation)

Snapshot of `frontend/src` before IPC consolidation. Kept for archaeology.

| File | invoke | listen / emit | window | dialog | os | Notes vs old `ipc/*` |
|------|--------|---------------|--------|--------|-----|----------------------|
| `shared/ipc/**` | yes | events helpers | — | dialogs | — | Canonical IPC_Layer |
| `shared/ipc/undo.ts` | yes | undo-state-changed | — | — | — | Track N undo/redo |
| `ipc/commands.ts` | re-export | — | — | — | — | Compat barrel → shared |
| `ipc/panelCommands.ts` | re-export | — | — | — | — | Compat barrel → shared |
| `hooks/useLayers.ts` | ~~raw~~ → layers/document | document-changed | — | — | — | Was duplicating add/remove/props |
| `hooks/useViewport.ts` | ~~raw~~ → viewport | — | — | — | — | Was raw `set_viewport` |
| `hooks/useSelectionState.ts` | ~~raw~~ → selection | selection-changed | — | — | — | Was raw get/set_selection |
| `hooks/useEffectLayer.ts` | ~~raw~~ → document | document-changed, panel-state | — | — | — | Snapshot + filters.update |
| `hooks/useDocumentState.ts` | ~~raw~~ → document | document-changed | — | — | — | |
| `hooks/useLayerState.ts` | ~~raw~~ → layers/document | document-changed | — | — | — | |
| `hooks/useDocument.ts` | via document / project | — | — | open/save | — | Open/create/save + path-parameterized Recent |
| `hooks/useRecentFiles.ts` | via recent | — | — | — | — | `get_recent_files` |
| `hooks/useWelcomeScreen.ts` | via useDocument + useRecentFiles | — | — | — | — | One Recent source + New Project per window |
| `hooks/usePanels.ts` | via panels | panel-state-changed | — | — | — | |
| `hooks/useCloseRequested.ts` | via panels | — | getCurrentWindow | — | — | Window chrome OK |
| `App.tsx` | ~~raw~~ → document/filters/palettes/panels | document-changed | — | — | — | Was raw snapshot |
| `components/PanelWindow.tsx` | ~~raw~~ → layers/viewport | listen/emit | getCurrentWindow | — | — | Duplicated set_layer_props / set_viewport |
| `components/TileCanvas.tsx` | — | tile events | — | — | — | No invoke |
| `components/*` (ColorLab, Palette*, filters) | via palettes/filters | — | — | open/save | — | |
| `components/AppTitlebar.tsx` / `WindowControls.tsx` | — | — | getCurrentWindow | — | — | Chrome exception |
| `lib/platform.ts` | — | — | — | — | platform() | OK outside domain IPC |
| `hooks/__tests__/**` | mocks | mocks | — | — | — | Allowed |

### Duplicates resolved in P0

- `get_document_snapshot`, `get_layer_tree`, `set_layer_props`, `set_viewport`, `set_selection` / `get_selection`, `remove_layer`, `reorder_layer`, `add_layer` — single wrappers in `shared/ipc`.
