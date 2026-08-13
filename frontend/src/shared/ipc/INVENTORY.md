# Tauri entrypoint inventory (P0 audit)

Snapshot of `frontend/src` before IPC consolidation. Domain `invoke` must live only under `shared/ipc/`.

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

## Duplicates resolved in P0

- `get_document_snapshot`, `get_layer_tree`, `set_layer_props`, `set_viewport`, `set_selection` / `get_selection`, `remove_layer`, `reorder_layer`, `add_layer` — single wrappers in `shared/ipc`.
