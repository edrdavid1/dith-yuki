# Implementation Plan: Color Frontend-Backend Integration

## Overview

Wire the existing frontend Color Lab modal and Effect Settings Panel to the already-implemented Tauri backend palette IPC commands. All work is frontend-only TypeScript/React. The implementation proceeds incrementally: hex utilities first, then PaletteSelector refresh support, then App.tsx apply wiring, then ColorLabWindow IPC integration, and finally DitherSettings live swatch display.

## Tasks

- [x] 1. Create hex conversion utility module
  - [x] 1.1 Create `frontend/src/utils/hexConvert.ts` with `hexToBackend` and `hexToDisplay` functions
    - `hexToBackend(displayHex: string): string` — strips "#" prefix and uppercases; throws on invalid input
    - `hexToDisplay(backendHex: string): string` — prepends "#" and lowercases; accepts with or without "#" prefix; throws on invalid input
    - Validate input using regex: must be 6 hex characters (optionally prefixed with "#" for `hexToDisplay`, must be prefixed for `hexToBackend`)
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ]* 1.2 Write property tests for hex conversion (Property 1: Hex format invariants)
    - **Property 1: Hex format conversion invariants**
    - Use `fast-check` to generate random valid 6-char hex strings [0-9A-F]
    - Assert `hexToBackend(hexToDisplay(s))` produces 6-char uppercase string equal to input uppercased
    - Assert `hexToDisplay(s)` produces 7-char string starting with "#" in all-lowercase hex
    - **Validates: Requirements 6.1, 6.2**

  - [ ]* 1.3 Write property test for hex round-trip (Property 2: Round-trip preservation)
    - **Property 2: Hex round-trip preservation**
    - Use `fast-check` to generate random 6-char uppercase hex strings
    - Assert `hexToBackend(hexToDisplay(x)) === x`
    - **Validates: Requirements 6.3**

  - [ ]* 1.4 Write property test for invalid hex rejection (Property 3: Invalid hex rejection)
    - **Property 3: Invalid hex rejection**
    - Use `fast-check` to generate random strings NOT matching valid hex patterns
    - Assert both `hexToBackend` and `hexToDisplay` throw an error
    - **Validates: Requirements 6.4**

- [x] 2. Add refreshKey prop to PaletteSelector
  - [x] 2.1 Add `refreshKey?: number` prop to `PaletteSelectorProps` interface and add `refreshKey` to the `useEffect` dependency array in `PaletteSelector.tsx`
    - This ensures `listPalettes()` is re-invoked whenever `refreshKey` changes
    - _Requirements: 7.1, 7.2_

  - [ ]* 2.2 Write unit test verifying PaletteSelector re-fetches when refreshKey changes
    - Mock `listPalettes` IPC, render PaletteSelector, change refreshKey prop, assert `listPalettes` called again
    - _Requirements: 7.1, 7.2_

- [x] 3. Wire handlePaletteApply in App.tsx
  - [x] 3.1 Add `paletteRefreshKey` and `lastCreatedPaletteId` state to App component and implement `handlePaletteApply` with IPC call
    - Add `const [paletteRefreshKey, setPaletteRefreshKey] = useState(0)`
    - Add `const [lastCreatedPaletteId, setLastCreatedPaletteId] = useState<number | null>(null)`
    - In `handlePaletteApply`: validate name (non-empty after trim), validate colors (at least one), call `addPalette(palette.name, palette.colors)`, on success store id + increment refreshKey + close modal, on error keep modal open and set error
    - Pass `paletteRefreshKey` to PaletteSelector instances (via EffectSettingsPanel)
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 7.1_

  - [ ]* 3.2 Write integration test for Apply flow (mocked IPC)
    - Mock `addPalette`, render App, open Color Lab, fill colors, press Apply, verify IPC called with correct args, modal closes, PaletteSelector refreshes
    - _Requirements: 1.1, 1.4, 1.6, 7.1_

- [x] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Wire ColorLabWindow extract buttons to generatePalette IPC
  - [x] 5.1 Replace `handleExtractFromRow` and `handleExtractFromActual` TODO stubs with real `generatePalette` IPC calls
    - Import `generatePalette` from `../ipc/commands`
    - Call `generatePalette(layerId!, extractCount, extractMethod)` on button press
    - On success: map `dto.colors` through `toHex(r, g, b)` to create `ColorEntry[]` and replace `colors` state
    - On error: display error message, preserve existing color list
    - Guard: if `layerId === null`, show "No image loaded" error (already partially done)
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [ ]* 5.2 Write unit test for extract flow (mocked IPC)
    - Mock `generatePalette`, render ColorLabWindow, click extract button, verify colors populated from response
    - _Requirements: 2.1, 2.2_

- [x] 6. Wire ColorLabWindow import buttons to Tauri file dialog + importPalette IPC
  - [x] 6.1 Replace `handleImport` TODO stub with Tauri file dialog open + `importPalette` IPC call
    - Import `open` from `@tauri-apps/plugin-dialog` and `importPalette` from `../ipc/commands`
    - Open file dialog with filters for `.ase`, `.aco`, `.gpl`, `.pal`, `.csv`, `.json`
    - On file selected: call `importPalette(filePath)`, map `dto.colors` through `toHex` to replace color list
    - On dialog cancelled: no-op
    - On error: display error message, preserve existing colors
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ]* 6.2 Write unit test for import flow (mocked dialog + IPC)
    - Mock `open` dialog and `importPalette`, verify color list populated from import response
    - _Requirements: 3.1, 3.3_

- [x] 7. Wire ColorLabWindow export buttons to validation + Tauri save dialog + addPalette + exportPalette IPC
  - [x] 7.1 Replace `handleExport` TODO stub with validation → save dialog → `addPalette` → `exportPalette` sequence
    - Import `save` from `@tauri-apps/plugin-dialog` and `addPalette`, `exportPalette` from `../ipc/commands`
    - Validate at least one valid color exists; if not, show "No colors to export." error
    - Open save dialog with extension filter matching selected format (default to "gpl" if unspecified)
    - On save path selected: call `addPalette("Export", validColors)`, then `exportPalette(dto.id, savePath, format)`
    - On success: show transient success notification
    - On dialog cancelled: no-op
    - On error: display error, preserve color list
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8_

  - [ ]* 7.2 Write unit test for export flow (mocked dialog + IPC)
    - Mock `save` dialog, `addPalette`, and `exportPalette`; verify full export sequence executes correctly
    - _Requirements: 4.3, 4.4, 4.5_

- [x] 8. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Replace hardcoded swatches in DitherSettings with live palette data
  - [x] 9.1 Remove `defaultSwatches` array and add palette data fetching to DitherSettings
    - Import `listPalettes` from `../ipc/commands` and `PaletteDto` type
    - Add `paletteRefreshKey?: number` prop to `EffectSettingsPanelProps` and pass through to DitherSettings
    - When `paletteId` is not null: fetch palette data via `listPalettes()` (or cache lookup), find the selected palette, render its `hex_colors` as swatches (prepend "#" for CSS background-color)
    - When `paletteId` is null: show "No palette selected" placeholder text
    - If palette has > 12 colors: render first 12 swatches + "+N" overflow indicator
    - If palette has 0 colors (empty): show empty swatch row
    - Re-fetch when `paletteRefreshKey` changes
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ]* 9.2 Write property test for swatch rendering order (Property 4: Swatch order preservation)
    - **Property 4: Swatch rendering order preservation**
    - Use `fast-check` to generate PaletteDto with 1-12 hex_colors
    - Assert rendered swatches match hex_colors in same order with "#" prefix
    - **Validates: Requirements 5.1**

  - [ ]* 9.3 Write property test for swatch overflow indicator (Property 5: Swatch overflow correctness)
    - **Property 5: Swatch overflow indicator correctness**
    - Use `fast-check` to generate PaletteDto with 13-256 hex_colors
    - Assert exactly 12 swatches rendered (first 12 in order) plus "+N" text where N = total - 12
    - **Validates: Requirements 5.4**

  - [ ]* 9.4 Write unit tests for DitherSettings swatch display edge cases
    - Test: when no palette selected, placeholder text shown
    - Test: when palette has 0 colors, empty swatch row rendered
    - Test: swatch updates when different palette selected
    - _Requirements: 5.2, 5.3, 5.5_

- [x] 10. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- All IPC commands already exist in `frontend/src/ipc/commands.ts` — no backend changes needed
- The Tauri file dialog plugin (`@tauri-apps/plugin-dialog`) must be available as a dependency
- `fast-check` is already in devDependencies for property-based testing

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "1.4", "2.2", "3.1"] },
    { "id": 2, "tasks": ["3.2", "5.1", "6.1", "7.1", "9.1"] },
    { "id": 3, "tasks": ["5.2", "6.2", "7.2", "9.2", "9.3", "9.4"] }
  ]
}
```
