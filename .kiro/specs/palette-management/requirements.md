# Requirements Document

## Introduction

This feature implements the full palette management workflow for Dither Yuki 2: Tauri IPC commands for CRUD operations on document palettes, individual color manipulation (add, remove, update, reorder), palette renaming, import/export via file dialogs, tile cache invalidation on palette changes, and the React UI components (PalettePanel, SwatchGrid, color picker integration). It builds on top of the existing `engine-color` palette data model and format parsers defined in the color-and-palette-engine spec.

## Glossary

- **Palette_Manager**: The set of Tauri IPC commands that expose palette CRUD operations to the frontend.
- **Document**: The engine-project Document struct containing layers, filters, and a collection of Palette entities.
- **Palette**: A named, ordered list of LinearColor entries stored in the Document, identified by PaletteId.
- **PaletteId**: A unique u32 identifier for a Palette within the Document.
- **LinearColor**: An f32 RGB color in linear color space (pre-linearized from sRGB).
- **Hex_Color**: A 6-character hexadecimal string representing an sRGB color (e.g., "FF0000" for red).
- **PaletteDto**: A serializable DTO sent to the frontend containing palette id, name, and colors as hex strings.
- **Palette_Panel**: The React sidebar component displaying the list of document palettes with action buttons.
- **Swatch_Grid**: The React component rendering palette colors as a grid of clickable/draggable square swatches.
- **Color_Picker**: A React color picker component (react-colorful or native input) for selecting colors in sRGB.
- **Invalidation_Cascade**: The process of marking Processed and Composite tiles dirty for all layers whose filters reference a modified palette.
- **PaletteKdCache**: The global DashMap-based cache of KD-trees keyed by (PaletteId, revision), already implemented in engine-color.
- **Filter_Reference**: A FilterInstance whose params contain a PaletteId (either PaletteQuantize or DitherV2 with palette_id set).
- **AppState**: The shared Tauri application state containing DocumentHandle, TileCache, Scheduler, and PaletteKdCache.

## Requirements

### Requirement 1: Create Palette via Tauri Command

**User Story:** As a user, I want to create a new named palette in the document, so that I can start building a custom color set for my dithering filters.

#### Acceptance Criteria

1. WHEN a `create_palette` command is invoked with a name string of 1 to 255 characters (inclusive, after trimming leading and trailing whitespace), THE Palette_Manager SHALL create a new Palette in the Document with an empty color list (0 colors), assign a PaletteId unique within the current document state, set the palette revision to 1, and return a PaletteDto containing the id (u32), name (string), an empty colors array, an empty hex_colors array, and color_count of 0.
2. IF the provided name, after trimming leading and trailing whitespace, is empty or exceeds 255 characters, THEN THE Palette_Manager SHALL return an error indicating the name length constraint violation without modifying the Document.
3. WHEN a palette is successfully created, THE Palette_Manager SHALL increment the Document revision by 1.
4. IF a palette with the same name already exists in the Document, THEN THE Palette_Manager SHALL still create the new palette (duplicate names are permitted), assigning a distinct PaletteId.

### Requirement 2: Delete Palette via Tauri Command

**User Story:** As a user, I want to delete a palette I no longer need, so that my document stays organized.

#### Acceptance Criteria

1. WHEN a `delete_palette` command is invoked with a PaletteId that exists and no FilterInstance references it, THE Palette_Manager SHALL remove the palette from the Document and return a success response with an empty list of affected filter IDs.
2. IF one or more FilterInstance entities reference the specified PaletteId, THEN THE Palette_Manager SHALL clear those palette references (set palette_id to None in DitherV2 params, remove PaletteQuantize filters from affected layers' filter stacks) and then remove the palette, returning success with a list of affected filter IDs.
3. IF the specified PaletteId does not exist in the Document, THEN THE Palette_Manager SHALL return an error indicating the palette was not found without modifying any state.
4. WHEN a palette is deleted and filters had references cleared, THE Palette_Manager SHALL trigger an Invalidation_Cascade for all affected layers by firing InvalidationEvent::LayerFilterChanged for each layer that contained a cleared reference.
5. THE Palette_Manager SHALL evict the deleted palette's entries from the PaletteKdCache keyed by (PaletteId, revision).
6. WHEN a palette is successfully deleted, THE Palette_Manager SHALL increment the Document revision by 1.

### Requirement 3: Add Color to Palette

**User Story:** As a user, I want to add a new color to an existing palette, so that I can expand my color set.

#### Acceptance Criteria

1. WHEN an `add_color_to_palette` command is invoked with a PaletteId and a Hex_Color string, THE Palette_Manager SHALL parse the hex string to sRGB u8 values, convert to LinearColor via sRGB-to-linear transfer function, append the color to the palette's color list, increment the palette revision, and return the updated PaletteDto.
2. IF the Hex_Color string is not a valid 6-character hexadecimal value, THEN THE Palette_Manager SHALL return an error indicating invalid color format.
3. IF the palette already contains 65536 colors, THEN THE Palette_Manager SHALL return an error indicating the maximum palette size has been reached.
4. IF the specified PaletteId does not exist, THEN THE Palette_Manager SHALL return an error indicating the palette was not found.
5. WHEN a color is added, THE Palette_Manager SHALL trigger an Invalidation_Cascade for all layers whose filters reference this PaletteId.

### Requirement 4: Update Color in Palette

**User Story:** As a user, I want to change an existing color in a palette, so that I can fine-tune my color choices.

#### Acceptance Criteria

1. WHEN an `update_palette_color` command is invoked with a PaletteId, a color index (usize), and a Hex_Color string, THE Palette_Manager SHALL parse the Hex_Color as a case-insensitive 6-character hexadecimal string, convert it to a LinearColor value, replace the color at the specified index, increment the palette revision by 1, and return the updated PaletteDto.
2. IF the specified PaletteId does not exist in the Document's palette collection, THEN THE Palette_Manager SHALL return an error indicating the palette was not found without modifying any state.
3. IF the color index is greater than or equal to the palette's current color count, THEN THE Palette_Manager SHALL return an error indicating the index is invalid.
4. IF the Hex_Color string is not exactly 6 characters or contains characters outside the ranges 0-9, a-f, and A-F, THEN THE Palette_Manager SHALL return an error indicating invalid color format.
5. WHEN a color is updated successfully, THE Palette_Manager SHALL trigger an Invalidation_Cascade that marks Processed and Composite tiles as dirty for all layers whose filters reference this PaletteId, and schedule re-rendering of affected viewport tiles.

### Requirement 5: Remove Color from Palette

**User Story:** As a user, I want to remove a color from a palette, so that I can trim colors I do not want.

#### Acceptance Criteria

1. WHEN a `remove_palette_color` command is invoked with a PaletteId and a color index, THE Palette_Manager SHALL remove the color at the specified index, shift subsequent colors down by one position, increment the palette revision by 1, and return the updated PaletteDto.
2. IF the specified PaletteId does not exist in the Document, THEN THE Palette_Manager SHALL return an error indicating the palette was not found without modifying any state.
3. IF the removal would leave the palette with zero colors AND one or more FilterInstance entities currently reference this palette, THEN THE Palette_Manager SHALL return an error indicating the palette cannot be empty while referenced by filters.
4. IF the removal would leave the palette with zero colors AND no FilterInstance entities reference this palette, THEN THE Palette_Manager SHALL permit the removal.
5. IF the color index is greater than or equal to the palette's current color count, THEN THE Palette_Manager SHALL return an error indicating the index is invalid.
6. WHEN a color is removed successfully, THE Palette_Manager SHALL trigger an Invalidation_Cascade for all layers whose filters reference this PaletteId.

### Requirement 6: Reorder Color in Palette

**User Story:** As a user, I want to drag and reorder colors within a palette, so that I can organize them visually.

#### Acceptance Criteria

1. WHEN a `reorder_palette_color` command is invoked with a PaletteId, a source index (from_index), and a destination index (to_index), THE Palette_Manager SHALL remove the color at from_index, insert it at to_index, shift remaining entries to accommodate the move, increment the palette revision, and return the updated PaletteDto containing the same set of colors as before the operation.
2. IF the specified PaletteId does not exist in the Document, THEN THE Palette_Manager SHALL return an error indicating the palette was not found.
3. IF either from_index or to_index is greater than or equal to the palette's current color count, THEN THE Palette_Manager SHALL return an error indicating the index is invalid.
4. IF from_index equals to_index, THEN THE Palette_Manager SHALL return the current PaletteDto unchanged without incrementing the revision or triggering invalidation.
5. WHEN colors are successfully reordered, THE Palette_Manager SHALL trigger an Invalidation_Cascade for all layers whose filters reference this PaletteId.

### Requirement 7: Rename Palette

**User Story:** As a user, I want to rename a palette, so that I can give it a meaningful name.

#### Acceptance Criteria

1. WHEN a `rename_palette` command is invoked with a PaletteId and a new name string (1–255 characters), THE Palette_Manager SHALL update the palette's name and return the updated PaletteDto containing the new name, even if the new name is identical to the current name or duplicates another palette's name.
2. IF the new name is empty, consists only of whitespace characters, or exceeds 255 characters, THEN THE Palette_Manager SHALL return an error indicating the name length constraint violation without modifying the palette.
3. IF the PaletteId does not exist, THEN THE Palette_Manager SHALL return an error indicating the palette was not found.
4. THE Palette_Manager SHALL NOT trigger an Invalidation_Cascade for rename operations because the name does not affect rendering output.

### Requirement 8: List Palettes

**User Story:** As a user, I want to see all palettes in my document, so that I can choose which one to edit or assign to a filter.

#### Acceptance Criteria

1. WHEN a `list_palettes` command is invoked, THE Palette_Manager SHALL return a vector of PaletteDto objects representing all palettes in the Document, ordered by their position in the document's palette collection. IF the Document contains zero palettes, THEN the returned vector SHALL be empty.
2. EACH PaletteDto SHALL contain: the palette id (u32), name (String), colors as a vector of 6-character uppercase hexadecimal strings (e.g. "FF0000") converted from LinearColor by clamping each channel to [0.0, 1.0] then applying the linear-to-sRGB transfer function, and a color_count field (usize) equal to the length of the colors vector.
3. WHEN converting LinearColor to Hex_Color for the PaletteDto, THE Palette_Manager SHALL clamp each linear RGB channel to [0.0, 1.0], apply the sRGB gamma encoding to produce a u8 per channel (0–255), and format the three bytes as a 6-character uppercase hexadecimal string without a "#" prefix.

### Requirement 9: Import Palette from File

**User Story:** As a user, I want to import a palette from a file (.ase, .gpl, .json), so that I can use palettes from external tools.

#### Acceptance Criteria

1. WHEN an `import_palette` command is invoked with a file path string, THE Palette_Manager SHALL detect the format from the file extension using case-insensitive matching, parse colors using engine-color's import_palette function, create a new Palette in the Document with the parsed colors, derive the palette name from the filename (without extension, truncated to 64 characters), and return the created PaletteDto.
2. THE Palette_Manager SHALL support the following file extensions for import: .ase (Adobe Swatch Exchange), .gpl (GIMP Palette), .json (JSON array), .aco (Adobe Color), .pal (Microsoft RIFF), and .csv (comma-separated values).
3. IF the file extension is not recognized or the file path has no extension, THEN THE Palette_Manager SHALL return an error indicating the format is unsupported.
4. IF parsing fails (invalid file content, sandbox violation, or empty palette), THEN THE Palette_Manager SHALL return a descriptive error from the underlying parser without creating a palette or modifying the Document.

### Requirement 10: Export Palette to File

**User Story:** As a user, I want to export a palette to a file, so that I can share it with other tools or back it up.

#### Acceptance Criteria

1. WHEN an `export_palette` command is invoked with a PaletteId, a file path, and a format string, THE Palette_Manager SHALL serialize the palette using engine-color's export_palette function, write the bytes to the specified path (overwriting any existing file at that path), and return a success result to the caller.
2. THE Palette_Manager SHALL support the following format strings for export using case-insensitive matching: "ase", "gpl", "json", "aco", "pal", and "csv".
3. IF the PaletteId does not exist, THEN THE Palette_Manager SHALL return an error indicating the palette was not found.
4. IF the format string is not recognized, THEN THE Palette_Manager SHALL return an error indicating the format is unsupported.
5. IF the file write fails (permission denied, invalid path, disk full), THEN THE Palette_Manager SHALL return an error indicating the I/O failure reason.
6. IF the palette identified by PaletteId contains zero colors, THEN THE Palette_Manager SHALL return an error indicating the palette is empty and cannot be exported.

### Requirement 11: Invalidation Cascade on Palette Modification

**User Story:** As the rendering system, I want all tiles affected by a palette change to be invalidated and recomputed, so that the canvas always reflects the current palette state.

#### Acceptance Criteria

1. WHEN a palette's color list is modified (add, remove, update, or reorder), THE Palette_Manager SHALL identify all FilterInstance entities in the Document that reference the modified PaletteId by searching DitherV2 params for matching palette_id and identifying PaletteQuantize filters referencing the palette.
2. FOR EACH layer containing an affected FilterInstance, THE Palette_Manager SHALL fire an InvalidationEvent::LayerFilterChanged for that layer, marking its Processed and Composite tiles dirty in the TileCache.
3. AFTER invalidation, THE Palette_Manager SHALL evict the stale (PaletteId, old_revision) entry from the PaletteKdCache and schedule recomputation of viewport-visible dirty tiles via the existing scheduler mechanism.
4. THE Palette_Manager SHALL NOT block on tile recomputation; invalidation, cache eviction, and scheduling SHALL complete synchronously and return to the caller.
5. IF no FilterInstance in the Document references the modified PaletteId, THEN THE Palette_Manager SHALL skip invalidation and scheduling entirely (no-op).

### Requirement 12: Palette Panel UI Component

**User Story:** As a user, I want a sidebar panel showing all my palettes with controls for creating, importing, exporting, and deleting them.

#### Acceptance Criteria

1. THE Palette_Panel SHALL display a scrollable list of all document palettes, showing each palette's name and a preview of its first 8 colors as square swatches (each swatch 16×16 CSS pixels), with the list scrolling vertically when palette entries exceed the visible panel height.
2. THE Palette_Panel SHALL provide buttons for: "Create Palette" (opens name input), "Import" (opens file dialog), "Export" (opens save dialog for selected palette), and "Delete" (removes selected palette after confirmation).
3. WHEN a palette is selected in the list, THE Palette_Panel SHALL highlight it with a visually distinct background and display the full Swatch_Grid editor for that palette below the list.
4. WHEN the "Create Palette" button is clicked and a name between 1 and 255 characters is entered, THE Palette_Panel SHALL invoke the `create_palette` Tauri command and append the new palette to the list.
5. IF the user submits an empty name or a name exceeding 255 characters in the create palette input, THEN THE Palette_Panel SHALL display an inline validation message indicating the name must be between 1 and 255 characters and SHALL NOT invoke the `create_palette` command.
6. WHEN the "Import" button is clicked, THE Palette_Panel SHALL open a native file dialog (via Tauri dialog API) filtered to supported palette formats (.ase, .aco, .gpl, .pal, .csv, .json), and invoke `import_palette` with the selected file path.
7. IF the user cancels the file dialog during import or export, THEN THE Palette_Panel SHALL close the dialog without modifying the palette list or document state.
8. IF the `import_palette` command returns an error, THEN THE Palette_Panel SHALL display an error notification indicating the import failure reason and SHALL NOT add a palette to the list.
9. WHEN the "Delete" button is clicked and a palette is selected, THE Palette_Panel SHALL display a confirmation prompt; upon user confirmation, THE Palette_Panel SHALL invoke `delete_palette` and remove the palette from the list.
10. WHILE no palette is selected in the list, THE Palette_Panel SHALL display the "Delete" and "Export" buttons in a disabled state.

### Requirement 13: Swatch Grid UI Component

**User Story:** As a user, I want to see and manipulate individual colors in a palette through a grid of color swatches.

#### Acceptance Criteria

1. THE Swatch_Grid SHALL render palette colors as square swatches in a wrapping grid layout that fills the available panel width, displaying the sRGB hex code (6-character uppercase, e.g. "FF0000") as a tooltip on hover.
2. WHEN a swatch is clicked once, THE Swatch_Grid SHALL select it (showing a visible border highlight), deselect any previously selected swatch, and enable the remove button.
3. WHEN a swatch is double-clicked, THE Swatch_Grid SHALL open the Color_Picker pre-filled with that swatch's current hex color.
4. THE Swatch_Grid SHALL provide a "+" button at the end of the grid that opens the Color_Picker for adding a new color.
5. WHEN a color is confirmed in the Color_Picker for adding a new color, THE Swatch_Grid SHALL invoke `add_color_to_palette` with the palette ID and the 6-character hex color value, and on success SHALL render the new swatch at the end of the grid.
6. WHEN a color is confirmed in the Color_Picker for editing an existing swatch, THE Swatch_Grid SHALL invoke `update_palette_color` with the palette ID, the swatch index, and the 6-character hex color value, and on success SHALL update the swatch display with the new color.
7. THE Swatch_Grid SHALL support drag-and-drop reordering of swatches, invoking `reorder_palette_color` with the source index and destination index when a drag completes at a different position.
8. WHEN the "−" (remove) button is clicked with a swatch selected, THE Swatch_Grid SHALL invoke `remove_palette_color` for the selected index, and on success SHALL deselect all swatches and disable the remove button.
9. IF any Tauri command invoked by the Swatch_Grid returns an error, THEN THE Swatch_Grid SHALL display an error message indicating the failure reason and preserve the grid state prior to the attempted operation.
10. WHILE the palette contains zero colors, THE Swatch_Grid SHALL display only the "+" button and the remove button in a disabled state.

### Requirement 14: Color Picker Integration

**User Story:** As a user, I want a color picker to select colors in sRGB for my palette, with the engine receiving them in linear space.

#### Acceptance Criteria

1. THE Color_Picker SHALL display an HSL/HSV color selection interface (via react-colorful HexColorPicker or equivalent library) as a modal overlay positioned near the triggering swatch.
2. WHEN the user interacts with the picker controls, THE Color_Picker SHALL display a live preview of the selected color as a 6-character uppercase hexadecimal string (without "#" prefix) in a text input field that is also directly editable.
3. THE Color_Picker SHALL support both "add new color" mode (no initial value, defaults to "FFFFFF") and "edit existing color" mode (pre-filled with the current swatch's hex color).
4. THE Color_Picker SHALL provide a "Confirm" button that closes the picker and emits the final 6-character hex string to the parent component, and a "Cancel" button that closes the picker without emitting any value.
5. WHEN the user clicks outside the Color_Picker modal or presses Escape, THE Color_Picker SHALL behave as if "Cancel" was pressed (close without emitting).

### Requirement 15: Palette-to-Filter Binding in UI

**User Story:** As a user, I want to assign a palette to a dither or palette-quantize filter from the filter panel, so that the filter uses my chosen color set.

#### Acceptance Criteria

1. WHEN a DitherV2 or PaletteQuantize filter is displayed in the filter panel, THE filter parameter UI SHALL show a palette selector dropdown listing all document palettes by name, ordered by PaletteId ascending.
2. WHEN a DitherV2 filter is displayed, THE palette selector dropdown SHALL include a "None" option as the first entry, representing no palette assignment (uniform quantization by levels).
3. WHEN a PaletteQuantize filter is displayed, THE palette selector dropdown SHALL NOT include a "None" option, since PaletteQuantize requires a palette reference.
4. WHEN a palette is selected from the dropdown for a DitherV2 filter, THE filter parameter UI SHALL invoke `update_filter` with the `palette_id` field set to the selected palette's PaletteId.
5. WHEN a palette is selected from the dropdown for a PaletteQuantize filter, THE filter parameter UI SHALL invoke `update_filter` with the `palette_id` field set to the selected palette's PaletteId.
6. WHEN "None" is selected in the dropdown for a DitherV2 filter, THE filter parameter UI SHALL invoke `update_filter` with `palette_id` set to null, reverting the filter to uniform quantization by levels.
7. IF the currently assigned palette is deleted and the filter is DitherV2, THEN THE filter parameter UI SHALL set the dropdown selection to "None" and invoke `update_filter` with `palette_id` set to null.
8. IF the currently assigned palette is deleted and the filter is PaletteQuantize, THEN THE filter parameter UI SHALL set the dropdown selection to the first available palette in the document and invoke `update_filter` with that palette's PaletteId. IF no other palettes exist in the document, THE filter parameter UI SHALL disable the filter and display an indication that no palette is available.

### Requirement 16: Hex-to-Linear Color Conversion in Tauri Commands

**User Story:** As the system, I want consistent hex-to-linear conversion in all palette commands, so that colors stored in the document match what the user selected in the UI.

#### Acceptance Criteria

1. WHEN a Hex_Color string is received by any palette Tauri command, THE Palette_Manager SHALL accept exactly 6 hexadecimal characters (0-9, a-f, A-F) with no "#" prefix, parse it as case-insensitive hexadecimal, extract R, G, B as u8 values, and convert each to linear f32 using the sRGB-to-linear transfer function from engine-color.
2. IF a Hex_Color string received by a palette Tauri command does not match the pattern of exactly 6 hexadecimal characters, THEN THE Palette_Manager SHALL reject the input and return an error indicating the string is not a valid 6-character hexadecimal color.
3. WHEN converting LinearColor to Hex_Color for PaletteDto responses, THE Palette_Manager SHALL apply the linear-to-sRGB transfer function to each channel, clamp to [0.0, 1.0] before conversion, round to nearest u8, and format as a 6-character uppercase hexadecimal string with no "#" prefix.
4. THE Palette_Manager SHALL maintain the round-trip property: for any valid 6-character hexadecimal input, converting to LinearColor via sRGB-to-linear and back to Hex_Color via linear-to-sRGB SHALL produce the same uppercase 6-character string.
