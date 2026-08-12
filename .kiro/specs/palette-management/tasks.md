# Implementation Plan: Palette Management

## Overview

This plan implements the full palette management feature for Dither Yuki 2: new Tauri commands for individual color manipulation (add, update, remove, reorder), palette create/rename/export, tile invalidation on palette changes, and React UI components (SwatchGrid, ColorPicker, enhanced PalettePanel, palette-filter binding).

## Tasks

- [x] 1. Implement hex color conversion utilities (`hex_to_linear`, `linear_to_hex`) in `src-tauri/src/commands.rs` with unit tests for valid/invalid inputs and case insensitivity
- [x] 2. Update `PaletteDto` struct to add `hex_colors: Vec<String>` field and update `palette_to_dto` to populate it using `linear_to_hex`
- [x] 3. Implement `find_layers_referencing_palette` helper that recursively walks layer tree collecting LayerIds of layers whose filters reference a given PaletteId (both DitherV2 and PaletteQuantize)
- [x] 4. Implement `invalidate_palette_changed` helper that calls `find_layers_referencing_palette`, fires `InvalidationEvent::LayerFilterChanged` per affected layer, and calls `schedule_dirty_viewport_tiles`
- [x] 5. Implement `create_palette` Tauri command: validate name (1–255 chars), call `doc.add_palette(name, vec![])`, increment generation, return PaletteDto; register in `main.rs`
- [x] 6. Implement `add_color_to_palette` Tauri command: parse hex via `hex_to_linear`, validate palette exists and size < 65536, push color, increment palette revision, call `invalidate_palette_changed`, return PaletteDto; register in `main.rs`
- [x] 7. Implement `update_palette_color` Tauri command: parse hex, validate palette exists and index in bounds, replace color at index, increment revision, call `invalidate_palette_changed`, return PaletteDto; register in `main.rs`
- [x] 8. Implement `remove_palette_color` Tauri command: validate palette exists, index in bounds, check if removal would empty a referenced palette (error), remove color, increment revision, call `invalidate_palette_changed`, return PaletteDto; register in `main.rs`
- [x] 9. Implement `reorder_palette_color` Tauri command: validate palette exists and indices in bounds, no-op if from==to, move element, increment revision, call `invalidate_palette_changed`, return PaletteDto; register in `main.rs`
- [x] 10. Implement `rename_palette` Tauri command: validate name (1–255 chars), validate palette exists, update name (no invalidation), return PaletteDto; register in `main.rs`
- [x] 11. Implement `export_palette` Tauri command: validate palette exists, parse format string to PaletteFormat, call `engine_color::palette::export_palette`, write bytes to file; register in `main.rs`
- [x] 12. Implement enhanced `delete_palette` Tauri command: find filter references, clear DitherV2 palette_id to None, remove PaletteQuantize filters from affected layers, remove palette, evict from PaletteKdCache, invalidate affected layers, return affected filter IDs; register in `main.rs`
- [x] 13. Add frontend IPC wrappers in `frontend/src/ipc/commands.ts`: `createPalette`, `deletePalette`, `addColorToPalette`, `updatePaletteColor`, `removePaletteColor`, `reorderPaletteColor`, `renamePalette`, `exportPalette`
- [x] 14. Install `react-colorful` dependency and create `frontend/src/components/ColorPicker.tsx` modal component with HexColorPicker, initialColor prop, Confirm/Cancel buttons
- [x] 15. Create `frontend/src/components/SwatchGrid.tsx` component: render color swatches as CSS Grid, implement click-to-select, double-click-to-edit, "+" to add, "−" to remove, and drag-and-drop reordering
- [x] 16. Enhance `frontend/src/components/PalettePanel.tsx`: add "Create Palette" button with name input, "Export" button per palette with save dialog, inline palette name editing, integrate SwatchGrid for selected palette, update delete to use new `deletePalette` command
- [x] 17. Create `PaletteSelector` dropdown component and integrate into DitherV2 and PaletteQuantize filter parameter panels, calling `updateFilter` on selection change
- [x] 18. Write Rust integration tests: full palette CRUD lifecycle, invalidation cascade verification (modify palette → verify tiles marked dirty), force-delete with filter reference clearing

## Task Dependency Graph

```json
{
  "waves": [
    {"tasks": ["1", "3", "5", "10", "11", "14"]},
    {"tasks": ["2", "4"]},
    {"tasks": ["6", "7", "8", "9", "12"]},
    {"tasks": ["13"]},
    {"tasks": ["15", "17"]},
    {"tasks": ["16"]},
    {"tasks": ["18"]}
  ]
}
```

## Notes

- The existing `remove_palette` command enforces referential integrity (fails if referenced). The new `delete_palette` (task 12) is a force-delete that clears references first. Both can coexist — `remove_palette` for programmatic use, `delete_palette` for UI use.
- `react-colorful` is a lightweight (~3KB) color picker that supports hex input/output directly, matching the project's minimal dependency approach.
- Drag-and-drop in SwatchGrid uses the HTML5 DnD API to avoid adding a library dependency. If experience is poor, can upgrade to `@dnd-kit` later.
- The `hex_colors` field in PaletteDto provides pre-formatted hex strings for the frontend, avoiding redundant conversion logic in TypeScript.
