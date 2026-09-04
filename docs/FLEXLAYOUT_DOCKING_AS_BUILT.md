# FlexLayout docking — as-built (index)

**Canonical doc (RU, full):** [`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md)  
**ADR:** [`B2_ADR_flexlayout_docking.md`](./B2_ADR_flexlayout_docking.md)  
**LICENSE snapshot:** [`legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md)  
**Date:** 2026-09-04 · **Scope:** Layers + Effect + Color Lab on FlexLayout; B4c JS popout drag (no `global_mouseup`).

---

## Snapshot

| Concern | Owner |
|--------|--------|
| Tab tree, same-side drag, float/popout | Two FlexLayout `Model`s (left + right) |
| Column width / collapse | `ShellContext` |
| Drag-to-redock | JS `setPosition` + in-WebView mouseup → `complete_float_drag`; hit-test via `dock_affinity` + zone IPC |
| Persist | Raw `toJson()` → `save_layout_{left,right}`; B4b injects missing `colorlab` |

**Migrated:** `layers`, `effect`, `colorlab`. **Not FL:** Preview / Preferences.

**Float:** patched FloatingWindow → Tauri `flex-popout-*` + `FlexPopoutChrome` (JS drag, not OS `startDragging`).  
**Redock:** close chrome, or drag onto dock (affinity). Discoverability when all tabs floated — QA.

**Library pin:** `flexlayout-react@0.7.15` — undocumented vs ADR ~0.10.x (open bump decision).

Spec: [`.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md`](../.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md). Full detail: **[`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md)** §§12–13.
