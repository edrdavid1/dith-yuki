# Requirements Document

## Introduction

This feature implements the `engine-color` crate and palette-aware filter pipeline for the Dither Yuki 2 dithering/image-processing engine. It covers Oklab color space conversions (operating on already-linearized RGB data from PixelTile), a document-level Palette entity with import/export/generation capabilities, a global DashMap-based KD-tree cache for concurrent palette lookups, two distinct filter kinds (Dither without palette and PaletteQuantize with palette), custom PNG threshold maps with sandbox path validation, and a shared `resolve_user_path` utility in `engine-io`.

## Glossary

- **Engine_Color**: The `engine-color` Rust crate responsible for Oklab color space conversions, KD-tree nearest-color search, palette parsing, palette generation, and threshold map loading.
- **Engine_IO**: The `engine-io` Rust crate responsible for file I/O and sandbox path validation utilities.
- **Engine_Project**: The `engine-project` Rust crate containing the Document model, filter dispatch, and layer/compositor logic.
- **PixelTile**: A 260×260 (256 + 2px halo) tile storing RGBA f32 data in linear RGB color space.
- **Oklab**: A perceptually uniform color space used for nearest-color search and error diffusion during palette quantization.
- **Linear_RGB**: The internal pixel representation (f32, pre-linearized sRGB) used by all tiles in the system.
- **KD_Tree**: A k-dimensional tree data structure for efficient nearest-neighbor color lookup in Oklab space.
- **Palette**: A named, ordered list of colors stored in linear RGB as a document-level entity, identified by PaletteId.
- **PaletteId**: A unique integer identifier referencing a Palette within the Document.
- **PaletteKdCache**: A global DashMap-based concurrent cache mapping (PaletteId, revision) to an Arc-wrapped KD-tree.
- **Dither_Filter**: A filter that performs color reduction (Bayer, Ordered, ThresholdMap, ErrorDiffusion) to N levels per channel without a specific palette.
- **PaletteQuantize_Filter**: A filter that quantizes tile pixels to the nearest color in a referenced Palette using Oklab distance and optional error diffusion.
- **ThresholdMap**: A grayscale PNG image used as a custom ordered-dither pattern, loaded with sandbox validation.
- **Sandbox_Validator**: The `resolve_user_path` function in `engine-io` that validates external file paths against allowed extensions and ensures paths reside within the user's home directory.
- **TileCoord**: A struct containing pyramid level, x, and y tile grid coordinates used for global coordinate computation.
- **DashMap**: A concurrent, sharded hash map used for lock-free read access across worker threads.
- **Error_Diffusion**: A dithering technique that distributes quantization error to neighboring pixels using kernel weights (Floyd-Steinberg, Atkinson, Jarvis-Judice-Ninke, Stucki).
- **Bayer_Matrix**: A deterministic threshold pattern (2×2, 4×4, 8×8) used for ordered dithering.

## Requirements

### Requirement 1: Oklab Color Space Conversion

**User Story:** As a rendering pipeline developer, I want to convert between linear RGB and Oklab color space, so that palette quantization and error diffusion operate in a perceptually uniform space.

#### Acceptance Criteria

1. WHEN a linear RGB pixel (f32 triplet) is provided, THE Engine_Color SHALL convert it to Oklab (L, a, b) using the standard Björn Ottosson LMS matrix and cube-root transfer function without applying sRGB linearization.
2. WHEN an Oklab triplet is provided, THE Engine_Color SHALL convert it back to linear RGB using the inverse cube and inverse LMS matrix, and SHALL clamp the resulting R, G, B values to [0.0, 1.0] before returning.
3. THE Engine_Color SHALL satisfy the round-trip property: for all linear RGB inputs where each channel is in [0.0, 1.0], converting to Oklab and back to linear RGB SHALL produce values within 1e-5 absolute tolerance per channel of the original input.
4. IF linear RGB input values fall outside [0.0, 1.0], THEN THE Engine_Color SHALL clamp each channel to [0.0, 1.0] before conversion to prevent undefined behavior in the cube-root step.
5. IF any input channel (R, G, B, L, a, or b) is NaN or infinity, THEN THE Engine_Color SHALL replace that channel with 0.0 before proceeding with conversion.
6. THE Engine_Color SHALL include source-level comments on the Oklab conversion functions stating that the LMS matrix assumes sRGB/Rec.709 primaries and that inputs from non-sRGB working spaces require prior ICC-based conversion to linear sRGB.

### Requirement 2: Palette as a Document-Level Entity

**User Story:** As a document author, I want palettes to be stored as explicit named entities in the Document, so that multiple filters can reference the same palette by ID without implicit parameter passing.

#### Acceptance Criteria

1. THE Document SHALL store a collection of Palette entities, each containing a unique PaletteId, a name (String, 1 to 255 characters), a vector of LinearColor entries (r, g, b as f32, minimum 1 entry, maximum 65536 entries), and a revision counter (u64).
2. WHEN a new Palette is added to the Document, THE Engine_Project SHALL assign a unique PaletteId and set the initial revision to 1.
3. WHEN a Palette's color list is modified (entries added, removed, reordered, or individual color values changed), THE Engine_Project SHALL increment the Palette's revision counter.
4. IF a Palette removal is requested and one or more FilterInstance entities reference that PaletteId, THEN THE Engine_Project SHALL reject the removal and return an error indicating which FilterInstance references exist.
5. WHEN a Palette removal is requested and no FilterInstance references the PaletteId, THE Engine_Project SHALL remove the Palette from the Document's collection.
6. THE Document serialization format SHALL include all Palette entities with their PaletteId, name, full color data, and revision counter, such that deserializing a serialized Document produces a Palette collection with identical field values.

### Requirement 3: Palette Import from File Formats

**User Story:** As a designer, I want to import palettes from standard file formats (.ase, .aco, .gpl, .pal, .csv, .json), so that I can use palettes from other tools.

#### Acceptance Criteria

1. WHEN a file path and format identifier are provided, THE Engine_Color SHALL parse the file and return a vector of (u8, u8, u8) sRGB color entries containing between 1 and 65,536 entries.
2. THE Engine_Color SHALL support parsing of Adobe Swatch Exchange (.ase), Adobe Color (.aco), GIMP Palette (.gpl), Microsoft RIFF Palette (.pal), comma-separated values (.csv), and JSON array (.json) formats.
3. WHEN parsing succeeds, THE Engine_Color SHALL convert each sRGB (u8) color to linear RGB (f32) by applying the sRGB transfer function for storage in the Palette.
4. IF a palette file contains invalid or corrupted data, THEN THE Engine_Color SHALL return a descriptive error indicating the format, byte offset or line number of the failure, and expected structure.
5. IF a palette file path fails sandbox validation, THEN THE Engine_Color SHALL return a SandboxError without attempting to read the file.
6. IF a palette file parses successfully but contains zero color entries, THEN THE Engine_Color SHALL return an error indicating that the palette is empty.
7. THE Engine_Color SHALL provide a pretty-printer for each supported format that converts a Palette back to the corresponding file bytes, producing output that is re-parseable by the same format's parser.
8. FOR ALL valid Palette instances, exporting to a format and re-importing SHALL produce an equivalent color list (round-trip property, with a maximum per-channel difference of 1 for formats that store colors as u8 values).

### Requirement 4: Palette Export to File Formats

**User Story:** As a designer, I want to export palettes to standard file formats, so that I can share palettes with other tools.

#### Acceptance Criteria

1. WHEN a Palette and target format are specified, THE Engine_Color SHALL serialize the Palette colors to the target format bytes, supporting the following formats: .gpl, .json, .ase, .aco, .pal, and .csv.
2. THE Engine_Color SHALL clamp each linear RGB (f32) color channel to the range [0.0, 1.0], then convert to sRGB (u8) by applying the inverse sRGB transfer function (gamma encoding), before writing to any export format.
3. WHEN the export format is .gpl, THE Engine_Color SHALL include the palette name (truncated to 256 characters if longer) as the GIMP Palette header name field.
4. WHEN the export format is .json, THE Engine_Color SHALL produce a JSON array of objects with "r", "g", "b" integer fields (0-255).
5. IF the specified target format is not one of the supported formats, THEN THE Engine_Color SHALL return an error indicating the format is unsupported without producing output bytes.
6. IF the Palette contains zero colors, THEN THE Engine_Color SHALL return an error indicating the palette is empty rather than producing an empty or malformed file.

### Requirement 5: Palette Generation from Layer

**User Story:** As a user, I want to generate a palette from an existing layer's pixel content, so that I can create palettes that match the image without manual color picking.

#### Acceptance Criteria

1. WHEN a layer ID, target color count (2-256), and generation method (MedianCut or KMeans) are provided, THE Engine_Color SHALL consume all non-fully-transparent pixels from the layer's tile data via the pixel iterator and produce a Palette with at most the specified number of distinct colors.
2. THE Engine_Color SHALL accept an iterator of linear RGB pixels (from tiles) rather than requiring access to the full TileCache, maintaining crate boundary separation.
3. WHEN the MedianCut method is selected, THE Engine_Color SHALL recursively subdivide the color space along the axis of greatest range until the target color count is reached, rounding down to the nearest achievable split count if the target is not a power of 2.
4. WHEN the KMeans method is selected, THE Engine_Color SHALL initialize centroids via k-means++ and iterate until the maximum Euclidean distance any centroid moves between iterations is less than 1e-4, or a maximum of 50 iterations is reached, whichever comes first.
5. WHEN palette generation completes successfully, THE generated Palette SHALL be stored in the Document's palette collection with a name in the format "{layer_name}_{method}" truncated to 64 characters maximum.
6. IF the specified layer ID does not exist or the layer contains no non-transparent pixels, THEN THE Engine_Color SHALL return an error indicating the reason without modifying the palette collection.
7. IF the layer contains fewer unique colors than the requested target count, THEN THE Engine_Color SHALL return a Palette containing only the unique colors found, with the actual count being less than the target.

### Requirement 6: Global KD-Tree Cache (PaletteKdCache)

**User Story:** As the tile rendering system, I want a global concurrent KD-tree cache keyed by PaletteId, so that multiple worker threads can perform nearest-color lookups without mutex contention.

#### Acceptance Criteria

1. THE PaletteKdCache SHALL store entries as a DashMap mapping PaletteId to a tuple of (revision: u64, Arc of KD_Tree).
2. WHEN a KD-tree is requested for a Palette whose PaletteId is present in the cache and whose stored revision equals the Palette's current `revision` field, THE PaletteKdCache SHALL return the existing cached Arc without rebuilding.
3. IF the stored revision does not match the Palette's current `revision` field or no entry exists for the PaletteId, THEN THE PaletteKdCache SHALL build a new KD-tree from the Palette's Oklab-converted colors, store it with the current revision, and return the new Arc.
4. IF two or more threads concurrently request a KD-tree for the same PaletteId when no valid cache entry exists, THEN THE PaletteKdCache SHALL allow concurrent builds and accept the last-writer-wins result stored in the DashMap, without blocking readers.
5. THE KD_Tree SHALL perform nearest-neighbor search using Euclidean (L2) distance in 3-dimensional Oklab space (L, a, b) and return the index of the closest palette color.
6. IF two or more palette colors are equidistant from the query point, THEN THE KD_Tree SHALL return the color with the lowest index in the Palette's color list.
7. IF a Palette contains zero colors, THEN THE PaletteKdCache SHALL not build a KD-tree and SHALL return an empty-state Arc that yields no match on nearest-neighbor queries.
8. THE PaletteKdCache SHALL be safe to access concurrently from multiple worker threads without serializing reads (lock-free read path via DashMap sharding).
9. WHEN a Palette is removed from the Document, THE PaletteKdCache SHALL evict the corresponding entry before the next cache lookup for that PaletteId can occur.

### Requirement 7: Dither Filter (Without Palette)

**User Story:** As an artist, I want to apply dithering effects (Bayer, ordered, custom threshold map, error diffusion) that reduce color depth to N levels per channel without needing a specific palette.

#### Acceptance Criteria

1. THE Dither_Filter SHALL support four dithering modes: Bayer (2×2, 4×4, 8×8 matrices), Ordered (NxN threshold map where N is between 2 and 64 inclusive), ThresholdMap (custom PNG with dimensions between 2×2 and 4096×4096 inclusive), and ErrorDiffusion (Floyd-Steinberg, Atkinson, Jarvis-Judice-Ninke, Stucki kernels).
2. WHEN applying Bayer or Ordered dithering, THE Dither_Filter SHALL compute global pixel coordinates using `tile_x * TILE_SIZE + local_x` and `tile_y * TILE_SIZE + local_y` to ensure seamless tiling across tile boundaries.
3. WHEN applying ErrorDiffusion, THE Dither_Filter SHALL process pixels left-to-right, top-to-bottom within the tile and truncate error propagation at tile boundaries without requiring halo data from adjacent tiles.
4. THE Dither_Filter SHALL quantize each RGB channel independently to the configured number of levels (2-256, derived from color_depth bits 1-8).
5. THE Dither_Filter SHALL preserve the alpha channel unmodified during all dithering operations.
6. WHEN the same tile coordinates and parameters are provided, THE Dither_Filter SHALL produce identical output (determinism property).
7. IF a color_depth value outside the range 1-8 is provided, THEN THE Dither_Filter SHALL reject the configuration with an error message indicating the valid range.
8. IF a ThresholdMap PNG file cannot be loaded or has dimensions exceeding 4096×4096, THEN THE Dither_Filter SHALL reject the configuration with an error message indicating the file constraint that was violated.
9. WHEN applying ThresholdMap dithering, THE Dither_Filter SHALL tile the custom PNG map across pixel coordinates using modulo wrapping so that the map repeats seamlessly regardless of image size.

### Requirement 8: PaletteQuantize Filter

**User Story:** As an artist, I want to quantize image colors to a specific palette using perceptually accurate Oklab distance, so that the result matches the palette while preserving visual quality through optional error diffusion.

#### Acceptance Criteria

1. THE PaletteQuantize_Filter SHALL reference a specific PaletteId from the Document's palette collection.
2. WHEN processing a tile, THE PaletteQuantize_Filter SHALL convert each pixel from linear RGB to Oklab, find the nearest palette color via KD_Tree lookup using Euclidean distance in Oklab space, and write the nearest palette color (in linear RGB) to the output tile.
3. IF error diffusion is enabled, THEN THE PaletteQuantize_Filter SHALL compute the quantization error in Oklab space and distribute it to neighboring pixels using the configured diffusion kernel (Floyd-Steinberg or Atkinson), truncating error propagation at tile boundaries without cross-tile transfer.
4. WHILE error diffusion is active, THE PaletteQuantize_Filter SHALL clamp accumulated Oklab values to L∈[0, 1], a∈[−0.5, 0.5], b∈[−0.5, 0.5] after each error accumulation step to prevent drift beyond valid gamut boundaries.
5. THE PaletteQuantize_Filter SHALL obtain the KD_Tree from the global PaletteKdCache, not from an instance-level Mutex.
6. IF the referenced PaletteId does not exist in the Document, THEN THE PaletteQuantize_Filter SHALL return an error indicating the palette was not found.
7. THE PaletteQuantize_Filter SHALL preserve the alpha channel unmodified.
8. FOR ALL tiles processed by the PaletteQuantize_Filter, every output pixel's RGB value SHALL exactly match one of the colors in the referenced Palette (palette membership invariant).
9. IF the referenced Palette contains zero colors, THEN THE PaletteQuantize_Filter SHALL return an error indicating the palette is empty rather than attempting KD_Tree construction.

### Requirement 9: Custom PNG Threshold Map

**User Story:** As an advanced user, I want to load a custom grayscale PNG as a threshold map for ordered dithering, so that I can create unique dithering patterns.

#### Acceptance Criteria

1. WHEN a custom threshold map path is configured on a Dither_Filter, THE Engine_Color SHALL load the PNG file, validate it as grayscale (1-bit or 8-bit), verify that dimensions do not exceed 4096×4096 pixels, and normalize pixel values to [0.0, 1.0] by dividing each sample by its bit-depth maximum (1 for 1-bit, 255 for 8-bit).
2. THE Engine_Color SHALL cache loaded threshold maps in a global DashMap keyed by (canonical path, file modification time), invalidating and reloading when the file's mtime changes, with a maximum of 64 cached entries evicting least-recently-used entries when full.
3. WHEN sampling the threshold map during dithering, THE Engine_Color SHALL use global pixel coordinates modulo the map dimensions to tile the pattern seamlessly.
4. IF the PNG file path fails sandbox validation, THEN THE Engine_Color SHALL return a SandboxError without attempting to read the file.
5. IF the PNG file is not a valid grayscale image (neither 1-bit nor 8-bit grayscale), THEN THE Engine_Color SHALL return an error indicating the actual color type encountered and the expected grayscale format.
6. IF the PNG file cannot be read due to I/O failure or contains a corrupt/unparseable PNG stream, THEN THE Engine_Color SHALL return an error indicating the I/O or decoding failure reason.
7. IF the PNG dimensions exceed 4096×4096 pixels, THEN THE Engine_Color SHALL return an error indicating the maximum allowed dimensions.

### Requirement 10: Sandbox Path Validation (resolve_user_path)

**User Story:** As a security-conscious system, I want all external file path parameters to be validated through a single shared utility, so that path traversal attacks and symlink escapes are prevented consistently.

#### Acceptance Criteria

1. THE Sandbox_Validator SHALL be implemented as `resolve_user_path(raw: &str, allowed_ext: &[&str]) -> Result<PathBuf, SandboxError>` in the `engine-io` crate.
2. WHEN a raw path is provided, THE Sandbox_Validator SHALL first verify that the raw path's file extension matches one of the allowed extensions (case-insensitive ASCII comparison), and then canonicalize the resolved path to verify home-directory containment.
3. THE Sandbox_Validator SHALL canonicalize the path (resolving symlinks and `..` components) and verify the resolved path starts with the user's home directory as returned by the platform home-directory lookup.
4. IF the raw path has no file extension or the extension is not in the allowed list, THEN THE Sandbox_Validator SHALL return a BadExtension error.
5. IF the canonicalized path does not reside within the user's home directory, THEN THE Sandbox_Validator SHALL return an OutsideHome error.
6. IF the path cannot be resolved (file not found, permission denied), THEN THE Sandbox_Validator SHALL return a NotFound error.
7. IF the user's home directory cannot be determined by the platform, THEN THE Sandbox_Validator SHALL return a NoHome error.
8. THE Engine_Color palette file importer and threshold map loader SHALL both use the Sandbox_Validator rather than implementing independent path checks.

### Requirement 11: Filter Kind Separation (Dither vs PaletteQuantize)

**User Story:** As an engine architect, I want Dither and PaletteQuantize to be distinct FilterKind variants with separate parameter structs, so that palette-free dithering does not carry unnecessary Oklab/KD-tree machinery and vice versa.

#### Acceptance Criteria

1. THE FilterKind enum SHALL include both `Dither` and `PaletteQuantize` as separate variants.
2. THE FilterParams for Dither SHALL contain: dither mode (one of Bayer, ThresholdMap, or ErrorDiffusion), a Bayer matrix size (one of 2, 4, or 8) when mode is Bayer, a threshold map path when mode is ThresholdMap, an error diffusion kernel (one of FloydSteinberg, Atkinson, JarvisJudiceNinke, or Stucki) when mode is ErrorDiffusion, and color_depth (u8, 1–8 bits).
3. THE FilterParams for PaletteQuantize SHALL contain: palette_id (PaletteId), an optional error diffusion kernel (None meaning nearest-color only, or one of FloydSteinberg, Atkinson, JarvisJudiceNinke, or Stucki), and no color_depth field.
4. WHEN the filter dispatcher encounters FilterKind::Dither, THE Engine_Project SHALL apply the Dither_Filter without consulting any Palette or KD_Tree.
5. WHEN the filter dispatcher encounters FilterKind::PaletteQuantize, THE Engine_Project SHALL obtain the referenced Palette from the Document and the KD_Tree from the PaletteKdCache before applying the filter.
6. IF the filter dispatcher encounters FilterKind::PaletteQuantize and the referenced palette_id does not exist in the Document's palette list, THEN THE Engine_Project SHALL return an error indicating the palette was not found and SHALL leave the tile unchanged.
7. THE Dither_Filter and PaletteQuantize_Filter SHALL both set `requires_full_row` to false, confirming they operate correctly within per-tile processing boundaries.

### Requirement 12: Crate Organization and Module Structure

**User Story:** As a maintainer, I want the engine-color crate to follow a clear module structure separating Oklab math, KD-tree, palette management, and threshold maps, so that the code is navigable and testable.

#### Acceptance Criteria

1. THE Engine_Color crate SHALL expose the following public modules via `pub mod` declarations in `lib.rs`: `oklab` (color space conversion between linear RGB and Oklab), `kdtree` (KD-tree construction and nearest-neighbor search), `palette` (Palette struct, file format parsers, and palette generators), and `palette_cache` (PaletteKdCache for concurrent KD-tree access).
2. THE Engine_Color crate SHALL expose a `threshold_map` module containing threshold map loading from PNG files and caching by file path and modification time.
3. THE Engine_IO crate SHALL expose a `sandbox` module containing the `resolve_user_path` function that validates file extensions and ensures paths resolve within the user's home directory.
4. THE Engine_Project crate's `filters/` directory SHALL contain `dither.rs` (Dither_Filter supporting Bayer, ordered, threshold-map, and error-diffusion algorithms without a palette) and a `palette_quantize.rs` (PaletteQuantize_Filter performing Oklab-based quantization against a referenced Palette via KD-tree lookup).
5. THE Engine_Color crate SHALL NOT list `engine-project` or `engine-tiles` in its `[dependencies]` section of `Cargo.toml`, ensuring it remains a leaf crate in the workspace dependency graph.
6. WHEN the workspace is compiled, THE Engine_IO crate SHALL NOT depend on `engine-project`, `engine-tiles`, or `engine-color` in its `[dependencies]` section of `Cargo.toml`, maintaining its role as an independent utility crate.
7. WHEN a developer runs `cargo test -p engine-color`, THE Engine_Color crate SHALL compile and execute unit tests for each public module (`oklab`, `kdtree`, `palette`, `palette_cache`, `threshold_map`) independently without requiring a running application or external service.
