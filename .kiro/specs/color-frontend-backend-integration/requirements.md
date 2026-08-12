# Requirements Document

## Introduction

This feature wires the Dither Yuki 2 React frontend Color Lab modal and Effect Settings Panel to the existing Tauri backend palette IPC commands. Currently, the ColorLabWindow uses TODO stubs (console.log) for palette creation, extraction, import, and export. The EffectSettingsPanel displays hardcoded placeholder swatches instead of real palette colors. The App.tsx `handlePaletteApply` callback is a no-op. This integration connects these frontend components to the fully implemented backend commands, including the necessary hex format conversion between the frontend's `#rrggbb` format and the backend's uppercase 6-character hex without prefix.

## Glossary

- **Color_Lab**: The ColorLabWindow modal component that provides manual palette creation, palette extraction from images, and import/export functionality.
- **Effect_Settings_Panel**: The EffectSettingsPanel component's DitherSettings sub-section that displays an "active color palette" swatch row and a PaletteSelector dropdown.
- **Backend**: The Tauri Rust backend with existing palette IPC commands (addPalette, generatePalette, importPalette, exportPalette, listPalettes, etc.).
- **Hex_Converter**: A utility module responsible for converting between the frontend `#rrggbb` hex format and the backend 6-character uppercase hex format without "#" prefix.
- **Palette_Data**: The data structure passed from Color_Lab on apply, containing a name (string) and colors (array of sRGB u8 triplets `[number, number, number][]`).
- **Palette_Dto**: The backend response structure containing id, name, colors (sRGB u8 triplets), hex_colors (6-char uppercase hex strings), and color_count.
- **File_Dialog**: The Tauri native file dialog API used to open or save files.
- **Swatch_Row**: The row of colored squares in DitherSettings that visually represents the active palette's colors.

## Requirements

### Requirement 1: Palette Apply Integration

**User Story:** As a user, I want to create a palette in Color Lab and have it saved to the document when I press Apply, so that the palette is available for dithering effects.

#### Acceptance Criteria

1. WHEN the user presses the Apply button in Color_Lab with at least one valid color in the color list, THE App SHALL invoke the `addPalette` IPC command with the palette name and sRGB u8 triplet array from Palette_Data.
2. IF the color list is empty or contains no valid colors when the user presses Apply, THEN THE Color_Lab SHALL display the error message "No valid colors to save." and SHALL NOT invoke the `addPalette` command.
3. IF the palette name is empty or consists only of whitespace, THEN THE Color_Lab SHALL display the error message "Palette name cannot be empty." and SHALL NOT invoke the `addPalette` command.
4. WHEN the `addPalette` IPC command returns successfully, THE App SHALL close the Color_Lab modal.
5. IF the `addPalette` IPC command returns an error, THEN THE Color_Lab SHALL display the error message within the modal and keep the Color_Lab modal open.
6. WHEN the `addPalette` IPC command returns successfully, THE App SHALL store the returned Palette_Dto id as the most recently created palette for downstream use.

### Requirement 2: Palette Extraction Integration

**User Story:** As a user, I want to extract a palette from the loaded image using Color Lab, so that I can generate palettes based on the image content without manually picking colors.

#### Acceptance Criteria

1. WHEN the user presses "Extract from row frame" or "Extract from actual frame" in Color_Lab, THE Color_Lab SHALL invoke the `generatePalette` IPC command with the current layerId, extractCount (integer between 2 and 256 inclusive), and extractMethod (one of "MedianCut" or "KMeans").
2. WHEN the `generatePalette` IPC command returns a Palette_Dto, THE Color_Lab SHALL replace its entire color list with ColorEntry objects created from the returned sRGB u8 triplets (converted to `#rrggbb` hex via the `toHex` utility), discarding any previously listed colors.
3. IF the layerId is null when extraction is triggered, THEN THE Color_Lab SHALL display the error message "No image loaded — cannot extract palette." and leave the existing color list unchanged.
4. IF the `generatePalette` IPC command returns an error, THEN THE Color_Lab SHALL display the error message to the user and leave the existing color list unchanged.

### Requirement 3: Palette Import Integration

**User Story:** As a user, I want to import palette files (ASE, GPL, HEX, JSON) into Color Lab, so that I can use pre-made palettes from external tools.

#### Acceptance Criteria

1. WHEN the user presses an Import button in Color_Lab, THE Color_Lab SHALL open a File_Dialog configured with file extension filters limited to the supported palette formats: `.ase`, `.aco`, `.gpl`, `.pal`, `.csv`, and `.json`.
2. WHEN the user selects a file in the File_Dialog, THE Color_Lab SHALL invoke the `importPalette` IPC command with the selected file path (the backend auto-detects format from the file extension).
3. WHEN the `importPalette` IPC command returns a Palette_Dto, THE Color_Lab SHALL replace its current color list with ColorEntry objects created from the returned sRGB u8 triplets (converted to `#rrggbb` hex via the `toHex` utility).
4. IF the user cancels the File_Dialog, THEN THE Color_Lab SHALL take no action and remain in its current state.
5. IF the `importPalette` IPC command returns an error, THEN THE Color_Lab SHALL display the error message to the user and retain the existing color list unchanged.

### Requirement 4: Palette Export Integration

**User Story:** As a user, I want to export the current Color Lab palette to a file, so that I can share or reuse palettes in other applications.

#### Acceptance Criteria

1. WHEN the user presses an Export button in Color_Lab, THE Color_Lab SHALL verify that the color list contains at least one valid color, where a valid color is a ColorEntry whose hex value is a parseable 6-character hexadecimal color string.
2. IF the color list is empty or contains no valid colors, THEN THE Color_Lab SHALL display the error message "No colors to export." and take no further export action.
3. WHEN the color list contains valid colors and the user has selected an export format (one of "ase", "aco", "gpl", "pal", "csv", or "json"), THE Color_Lab SHALL open a File_Dialog in save mode with the file extension filter corresponding to the selected format.
4. WHEN the user selects a save path in the File_Dialog, THE Color_Lab SHALL invoke the `addPalette` IPC command with the name "Export" and the current valid sRGB u8 triplet colors, then invoke the `exportPalette` IPC command with the returned palette id, the selected file path, and the selected format string.
5. WHEN the `exportPalette` IPC command completes successfully, THE Color_Lab SHALL display a transient success notification message indicating the file was saved.
6. IF the user cancels the File_Dialog, THEN THE Color_Lab SHALL take no action and remain in its current state with the color list preserved.
7. IF any IPC command in the export sequence returns an error, THEN THE Color_Lab SHALL display the error message to the user and preserve the current color list so the user may retry.
8. IF the user has not selected an export format when pressing the Export button, THEN THE Color_Lab SHALL default to the "gpl" format.

### Requirement 5: Active Palette Swatch Display

**User Story:** As a user, I want to see the actual colors of my selected palette in the Effect Settings dither section, so that I have visual confirmation of which palette is applied to the effect.

#### Acceptance Criteria

1. WHILE a palette is selected in the DitherSettings PaletteSelector (palette_id is not null), THE Effect_Settings_Panel SHALL display the selected palette's hex_colors as colored swatches in the Swatch_Row, rendered in the same order as they appear in the Palette_Dto hex_colors array, with each swatch's background color set to the corresponding hex value prefixed with "#".
2. WHILE no palette is selected in the DitherSettings PaletteSelector (palette_id is null), THE Effect_Settings_Panel SHALL display placeholder text indicating no palette is selected in place of the Swatch_Row.
3. WHEN the user selects a different palette in the PaletteSelector, THE Effect_Settings_Panel SHALL update the Swatch_Row to display the newly selected palette's colors without requiring a page reload.
4. IF the selected palette contains more than 12 colors, THEN THE Effect_Settings_Panel SHALL display only the first 12 swatches in the Swatch_Row followed by a text indicator showing "+N" where N is the total color count minus 12.
5. WHEN the selected palette's hex_colors array is empty (color_count is 0), THE Effect_Settings_Panel SHALL display an empty Swatch_Row with no swatch elements and no overflow indicator.

### Requirement 6: Hex Format Conversion

**User Story:** As a developer, I want a consistent hex format conversion utility, so that all IPC calls correctly translate between the frontend `#rrggbb` display format and the backend 6-character uppercase format.

#### Acceptance Criteria

1. THE Hex_Converter SHALL provide a function that converts a `#rrggbb` or `#RRGGBB` string to a 6-character uppercase string without the "#" prefix.
2. THE Hex_Converter SHALL provide a function that converts a 6-character hex string (with or without "#" prefix) to a lowercase `#rrggbb` string for frontend display.
3. FOR ALL valid 6-character hex strings, converting from backend format to display format and back to backend format SHALL produce the original uppercase string (round-trip property).
4. IF a string passed to Hex_Converter is not a valid hex color, THEN THE Hex_Converter SHALL throw a descriptive error.

### Requirement 7: Palette Selector Refresh on Apply

**User Story:** As a user, I want the PaletteSelector dropdown to show my newly created palette immediately after applying it from Color Lab, so that I can select it for my dither effect without reloading.

#### Acceptance Criteria

1. WHEN a palette is successfully created via the Apply flow in Color_Lab, THE PaletteSelector component SHALL refresh its palette list to include the newly created palette.
2. THE PaletteSelector SHALL display the refreshed list within the same user interaction cycle (no manual page reload required).
