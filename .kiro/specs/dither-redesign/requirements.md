# Requirements Document

## Introduction

This document specifies the redesign of the dithering system in Dither Yuki 2. The current implementation uses a legacy `DitherFilter` with `color_depth` (1–8 bits) and a limited `DitherMode` enum. The redesign introduces the full parameter set described in the project's dithering specification (`dethering.md`): `levels`, `threshold_scale`, `pixel_size`, `color_mode`, and `palette_id` — integrating both ordered dithering and error diffusion as a unified non-destructive filter within the tile-based architecture.

The redesign also introduces cross-tile error propagation via `ErrorResiduals` buffers for error diffusion modes, ensuring pixel-perfect seam continuity that the current implementation lacks.

## Glossary

- **Dither_Filter**: The non-destructive filter that applies color reduction with artistic dithering patterns to a layer's tile data.
- **Ordered_Dithering_Engine**: The sub-component responsible for Bayer matrix and custom threshold map dithering algorithms.
- **Error_Diffusion_Engine**: The sub-component responsible for Floyd-Steinberg, Atkinson, and other kernel-based error diffusion algorithms.
- **Tile_Pipeline**: The system that processes raw tiles through the filter stack to produce processed tiles.
- **ErrorResiduals_Buffer**: A per-tile-edge data structure that stores quantization error residuals (right and bottom edges) for cross-tile error propagation.
- **Threshold_Map**: A grayscale PNG image used as a custom threshold pattern for ordered dithering.
- **KD_Tree**: A spatial data structure in Oklab space used for nearest-color palette lookup.
- **Palette_Cache**: The concurrent cache providing pre-built KD-trees for palette lookups.
- **Pixel_Size**: The block size parameter that groups multiple pixels into one "mega-pixel" for retro/pixel-art effects.
- **Quantization_Levels**: The number of discrete color levels per channel (2–256) used in uniform quantization mode.

## Requirements

### Requirement 1: Dither Filter Parameter Model

**User Story:** As an artist, I want the dither filter to expose all artistic parameters (mode, levels, threshold_scale, pixel_size, color_mode, palette_id), so that I can achieve a wide range of dithering effects from subtle texture to aggressive pixel-art.

#### Acceptance Criteria

1. THE Dither_Filter SHALL accept a `mode` parameter with values `"bayer_2x2"`, `"bayer_4x4"`, `"bayer_8x8"`, `"custom_png"`, `"floyd_steinberg"`, and `"atkinson"`
2. THE Dither_Filter SHALL accept a `levels` parameter as an integer in the range 2 to 256 inclusive
3. THE Dither_Filter SHALL accept a `threshold_scale` parameter as a float in the range 0.1 to 4.0 inclusive with a default of 1.0
4. THE Dither_Filter SHALL accept a `pixel_size` parameter as an integer in the range 1 to 32 inclusive with a default of 1
5. THE Dither_Filter SHALL accept a `color_mode` parameter with values `"rgb"` and `"grayscale"`
6. THE Dither_Filter SHALL accept a nullable `palette_id` parameter referencing a document palette
7. THE Dither_Filter SHALL accept a `custom_path` parameter for mode `"custom_png"` specifying the path to a grayscale PNG threshold map
8. IF the `mode` parameter value is not one of the defined values, THEN THE Dither_Filter SHALL return a validation error
9. IF the `levels` parameter is outside the range 2 to 256, THEN THE Dither_Filter SHALL return a validation error
10. IF the `threshold_scale` parameter is outside the range 0.1 to 4.0, THEN THE Dither_Filter SHALL return a validation error
11. IF the `pixel_size` parameter is outside the range 1 to 32, THEN THE Dither_Filter SHALL return a validation error
12. IF the `palette_id` is set and references a palette not present in the document, THEN THE Dither_Filter SHALL return an error at apply time

### Requirement 2: Ordered Dithering with Seamless Tile Patterns

**User Story:** As an artist, I want ordered dithering patterns to tile seamlessly across tile boundaries, so that the dither effect appears continuous regardless of how the image is divided into tiles.

#### Acceptance Criteria

1. WHEN the `mode` is `"bayer_2x2"`, `"bayer_4x4"`, or `"bayer_8x8"`, THE Ordered_Dithering_Engine SHALL compute the threshold matrix index using global pixel coordinates: `global_x = tile_coord.x * TILE_SIZE + local_x`, `global_y = tile_coord.y * TILE_SIZE + local_y`
2. WHEN the `mode` is `"custom_png"`, THE Ordered_Dithering_Engine SHALL sample the threshold map using global pixel coordinates via `rem_euclid` wrapping
3. THE Ordered_Dithering_Engine SHALL produce identical output for a given pixel regardless of which tile contains that pixel
4. THE Ordered_Dithering_Engine SHALL support fully parallel processing across all tiles without cross-tile dependencies
5. WHEN `threshold_scale` is 1.0, THE Ordered_Dithering_Engine SHALL apply the threshold offset as `(threshold - 0.5)` to the quantization decision
6. WHEN `threshold_scale` is not 1.0, THE Ordered_Dithering_Engine SHALL multiply the threshold offset by `threshold_scale` before applying it to the quantization decision

### Requirement 3: Error Diffusion with Cross-Tile Propagation

**User Story:** As an artist, I want error diffusion dithering to produce seamless results across tile boundaries, so that there are no visible seams or discontinuities in the dither pattern.

#### Acceptance Criteria

1. WHEN the `mode` is `"floyd_steinberg"` or `"atkinson"`, THE Error_Diffusion_Engine SHALL process pixels sequentially from left to right, top to bottom within a tile
2. THE Error_Diffusion_Engine SHALL use a 2-pixel HALO region read from adjacent raw tiles to seed error propagation at tile edges
3. WHEN a tile has been processed, THE Error_Diffusion_Engine SHALL store quantization error residuals from the right edge (2 columns) and bottom edge (2 rows) into the ErrorResiduals_Buffer keyed by tile coordinate
4. WHEN processing a tile that has a left or top neighbor, THE Error_Diffusion_Engine SHALL read the ErrorResiduals_Buffer of adjacent tiles to initialize error for boundary pixels
5. THE Error_Diffusion_Engine SHALL produce pixel-perfect results matching a hypothetical full-image error diffusion pass for tiles processed in row-major order
6. IF an adjacent tile's ErrorResiduals_Buffer is not yet available, THEN THE Error_Diffusion_Engine SHALL process tiles in row-major order to guarantee dependency availability

### Requirement 4: Pixel Size Block Quantization

**User Story:** As an artist, I want the pixel_size parameter to create retro pixel-art effects by treating blocks of pixels as one unit, so that I can achieve coarse mosaic-like dithering at various scales.

#### Acceptance Criteria

1. WHEN `pixel_size` is greater than 1, THE Dither_Filter SHALL group pixels into blocks of `pixel_size × pixel_size`
2. WHEN `pixel_size` is greater than 1, THE Dither_Filter SHALL compute the dithered color once per block using the block's representative pixel (top-left corner of the block in global coordinates)
3. THE Dither_Filter SHALL apply the computed block color uniformly to all pixels within the block
4. THE Dither_Filter SHALL align pixel blocks to global coordinates so that block boundaries remain consistent across tiles
5. WHEN `pixel_size` is 1, THE Dither_Filter SHALL process each pixel independently with no block grouping

### Requirement 5: Color Mode Processing

**User Story:** As an artist, I want to choose between RGB and grayscale dithering modes, so that I can either dither each color channel independently or work with a single luminance channel.

#### Acceptance Criteria

1. WHEN `color_mode` is `"rgb"`, THE Dither_Filter SHALL quantize each of the R, G, and B channels independently using the same levels and threshold settings
2. WHEN `color_mode` is `"grayscale"`, THE Dither_Filter SHALL convert the pixel to luminance using the formula `L = 0.2126 * R + 0.7152 * G + 0.0722 * B`, apply dithering to the single luminance channel, and write the result to all three RGB channels
3. THE Dither_Filter SHALL preserve the alpha channel unmodified regardless of the `color_mode` setting

### Requirement 6: Palette-Constrained Quantization

**User Story:** As an artist, I want to dither my image to match a specific color palette, so that I can emulate limited-color displays, print processes, or retro hardware.

#### Acceptance Criteria

1. WHEN `palette_id` is set, THE Dither_Filter SHALL ignore the `levels` parameter and quantize each pixel to the nearest color in the referenced palette
2. WHEN `palette_id` is set, THE Dither_Filter SHALL perform nearest-color lookup using the KD_Tree in Oklab perceptual color space
3. WHEN `palette_id` is set and `mode` is an ordered dithering mode, THE Dither_Filter SHALL apply the threshold offset to the pixel color before performing the nearest-palette lookup
4. WHEN `palette_id` is set and `mode` is an error diffusion mode, THE Error_Diffusion_Engine SHALL compute and distribute error in Oklab space between the adjusted pixel and the quantized palette color
5. WHEN `palette_id` is null, THE Dither_Filter SHALL perform uniform quantization using the `levels` parameter

### Requirement 7: Uniform Quantization

**User Story:** As an artist, I want to reduce the number of color levels per channel with precise control, so that I can create posterization and color banding effects at any granularity.

#### Acceptance Criteria

1. WHEN `palette_id` is null, THE Dither_Filter SHALL quantize pixel values to evenly spaced levels: `quantized = round(value * (levels - 1)) / (levels - 1)`
2. WHEN `levels` is 2, THE Dither_Filter SHALL produce a binary (two-tone) output per channel
3. WHEN `levels` is 256, THE Dither_Filter SHALL produce output visually identical to the input (no visible quantization)
4. THE Dither_Filter SHALL clamp quantized values to the range 0.0 to 1.0

### Requirement 8: Custom Threshold Map Support

**User Story:** As an artist, I want to use custom PNG images as threshold maps for ordered dithering, so that I can create unique artistic dither patterns beyond standard Bayer matrices.

#### Acceptance Criteria

1. WHEN `mode` is `"custom_png"`, THE Ordered_Dithering_Engine SHALL load the threshold map from the path specified in `custom_path`
2. THE Ordered_Dithering_Engine SHALL validate that the threshold map is a grayscale PNG (1-bit or 8-bit)
3. THE Ordered_Dithering_Engine SHALL validate that the threshold map dimensions do not exceed 4096×4096 pixels
4. THE Ordered_Dithering_Engine SHALL normalize threshold map pixel values to the range 0.0 to 1.0
5. THE Ordered_Dithering_Engine SHALL cache loaded threshold maps by (canonical_path, modification_time) to avoid redundant disk reads
6. IF the `custom_path` file does not exist or is not a valid grayscale PNG, THEN THE Ordered_Dithering_Engine SHALL return a descriptive error
7. IF the `custom_path` fails sandbox validation, THEN THE Ordered_Dithering_Engine SHALL return a security error

### Requirement 9: Filter Pipeline Integration

**User Story:** As a developer, I want the redesigned dither filter to integrate cleanly into the existing tile-based filter pipeline, so that dithering works as a composable non-destructive filter alongside curves, levels, and other effects.

#### Acceptance Criteria

1. THE Dither_Filter SHALL implement the same `apply_single_filter` interface used by curves, levels, glitch, and palette_quantize filters
2. THE Dither_Filter SHALL accept `TileCoord`, `PaletteKdCache`, `ThresholdMapCache`, and `Document` references consistent with the existing filter dispatcher signature
3. THE Dither_Filter SHALL be invocable at any position in a layer's filter stack
4. WHEN the Dither_Filter completes processing, THE Tile_Pipeline SHALL store the result as a Processed-stage tile in the tile cache
5. THE Dither_Filter SHALL be deterministic: applying the same parameters to the same input tile and coordinate SHALL always produce identical output

### Requirement 10: Error Diffusion Processing Order

**User Story:** As a developer, I want the system to handle error diffusion's sequential processing constraint within the parallel tile architecture, so that correctness is maintained without sacrificing responsiveness.

#### Acceptance Criteria

1. WHEN the `mode` is an error diffusion mode, THE Tile_Pipeline SHALL process tiles along rows (left to right, top to bottom) to satisfy cross-tile error dependencies
2. WHEN the `mode` is an ordered dithering mode, THE Tile_Pipeline SHALL allow fully parallel processing of all tiles without ordering constraints
3. THE Tile_Pipeline SHALL mark error diffusion filter instances with `requires_full_row = true` to signal the scheduler about processing order constraints
4. IF a tile depends on ErrorResiduals from a not-yet-processed neighbor, THEN THE Tile_Pipeline SHALL defer processing of that tile until the dependency is satisfied

### Requirement 11: Dither Filter Serialization

**User Story:** As a developer, I want the dither filter parameters to serialize and deserialize correctly, so that projects can be saved and loaded with dither settings preserved.

#### Acceptance Criteria

1. THE Dither_Filter SHALL serialize all parameters (`mode`, `levels`, `threshold_scale`, `pixel_size`, `color_mode`, `palette_id`, `custom_path`) to JSON-compatible format via Serde
2. WHEN a serialized Dither_Filter is deserialized, THE Dither_Filter SHALL restore all parameter values identically to their pre-serialization state (round-trip property)
3. IF a deserialized parameter value is outside its valid range, THEN THE Dither_Filter SHALL return a validation error on the next `validate()` call

### Requirement 12: Backward Compatibility

**User Story:** As a developer, I want existing projects that use the legacy `DitherFilter` (with `color_depth` 1–8 bits) to continue working after the redesign, so that no user data is broken by the upgrade.

#### Acceptance Criteria

1. WHEN the Tile_Pipeline encounters a legacy `FilterParams::Dither { mode: DitherMode, color_depth }` during deserialization, THE Dither_Filter SHALL convert it to the new parameter model by mapping `color_depth` to `levels = 2^color_depth`
2. WHEN a legacy `DitherMode::Bayer { matrix_size: 2 }` is encountered, THE Dither_Filter SHALL map it to mode `"bayer_2x2"` with default `threshold_scale = 1.0`, `pixel_size = 1`, `color_mode = "rgb"`, and `palette_id = null`
3. WHEN a legacy `DitherMode::ErrorDiffusion { kernel }` is encountered, THE Dither_Filter SHALL map it to the corresponding new mode (`"floyd_steinberg"` or `"atkinson"`) with default parameters
4. WHEN a legacy `DitherMode::ThresholdMap { path }` is encountered, THE Dither_Filter SHALL map it to mode `"custom_png"` with the same path in `custom_path`
