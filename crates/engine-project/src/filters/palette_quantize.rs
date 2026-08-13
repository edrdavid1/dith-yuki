//! PaletteQuantize filter implementation.
//!
//! Oklab-based palette quantization with optional error diffusion.
//! Converts each pixel to Oklab space, finds the nearest palette color
//! via KD-tree lookup, and writes the exact palette color to the output.
//!
//! When error diffusion is enabled, quantization error is distributed to
//! neighboring pixels in Oklab space before they are quantized.

use crate::error::EngineError;
use crate::filter::DiffusionKernel;
use engine_color::oklab::{linear_to_oklab, Oklab};
use engine_color::palette::Palette;
use engine_color::palette_lut::PaletteLut3D;
use engine_tiles::tile::PixelTile;
use engine_tiles::types::TileCoord;
use engine_tiles::{HALO, TILE_SIZE};

/// The full tile dimension including halo on each side.
const FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// PaletteQuantize filter: maps each pixel to the nearest palette color
/// in Oklab perceptual space, with optional error diffusion.
pub struct PaletteQuantizeFilter;

impl PaletteQuantizeFilter {
    /// Apply palette quantization to a tile.
    ///
    /// # Arguments
    ///
    /// * `tile` - Input pixel tile (260×260 RGBA, linear RGB)
    /// * `_coord` - Tile coordinate (unused, included for API consistency)
    /// * `palette` - The palette to quantize against
    /// * `lut` - Prebuilt Oklab 3D LUT for O(1) nearest-color lookup
    /// * `diffusion` - Optional error diffusion kernel
    ///
    /// # Returns
    ///
    /// A new `PixelTile` where every pixel's RGB exactly matches a palette entry.
    /// Alpha is preserved unmodified from the input.
    ///
    /// # Errors
    ///
    /// Returns `EngineError::InvalidFilterParams` if the palette is empty.
    pub fn apply(
        tile: &PixelTile,
        _coord: TileCoord,
        palette: &Palette,
        lut: &PaletteLut3D,
        diffusion: Option<DiffusionKernel>,
    ) -> Result<PixelTile, EngineError> {
        if palette.colors.is_empty() {
            return Err(EngineError::invalid_filter_params(
                "PaletteQuantize requires a non-empty palette",
            ));
        }

        match diffusion {
            None => Self::apply_nearest(tile, palette, lut),
            Some(kernel) => Self::apply_diffusion(tile, palette, lut, kernel),
        }
    }

    /// Nearest-color quantization (no error diffusion).
    ///
    /// For each pixel:
    /// 1. Read linear RGB from tile
    /// 2. Convert to Oklab
    /// 3. Find nearest palette index via LUT
    /// 4. Write palette color (linear RGB) to output
    /// 5. Copy alpha unchanged
    fn apply_nearest(
        tile: &PixelTile,
        palette: &Palette,
        lut: &PaletteLut3D,
    ) -> Result<PixelTile, EngineError> {
        let mut output = PixelTile::new();

        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                let r = tile.at(x, y, 0);
                let g = tile.at(x, y, 1);
                let b = tile.at(x, y, 2);

                let oklab = linear_to_oklab(engine_color::oklab::LinRgb { r, g, b });
                let nearest_idx = lut.nearest_index(oklab) as usize;
                let palette_color = &palette.colors[nearest_idx];

                output.set(x, y, 0, palette_color.r);
                output.set(x, y, 1, palette_color.g);
                output.set(x, y, 2, palette_color.b);
                // Preserve alpha unmodified
                output.set(x, y, 3, tile.at(x, y, 3));
            }
        }

        Ok(output)
    }

    /// Error diffusion quantization in Oklab space.
    ///
    /// 1. Allocate Oklab error buffer (260×260×3, initialized to 0.0)
    /// 2. For each pixel left-to-right, top-to-bottom:
    ///    - Convert pixel to Oklab
    ///    - Add accumulated error from buffer
    ///    - Clamp: L∈[0,1], a∈[-0.5,0.5], b∈[-0.5,0.5]
    ///    - Find nearest via LUT
    ///    - Compute error = adjusted - nearest_oklab
    ///    - Distribute error to neighbors via kernel
    ///    - Write nearest palette color (linear RGB) to output
    /// 3. Alpha preserved unmodified
    fn apply_diffusion(
        tile: &PixelTile,
        palette: &Palette,
        lut: &PaletteLut3D,
        kernel: DiffusionKernel,
    ) -> Result<PixelTile, EngineError> {
        let size = FULL_SIZE as usize;
        let mut output = PixelTile::new();

        // Error buffer: 3 channels (L, a, b) for each pixel
        let mut error_buf = vec![0.0f32; size * size * 3];

        // Pre-convert palette colors to Oklab for error computation
        let palette_oklab: Vec<Oklab> = palette
            .colors
            .iter()
            .map(|c| linear_to_oklab(engine_color::oklab::LinRgb { r: c.r, g: c.g, b: c.b }))
            .collect();

        for y in 0..size {
            for x in 0..size {
                let xu = x as u32;
                let yu = y as u32;

                // Read pixel and convert to Oklab
                let r = tile.at(xu, yu, 0);
                let g = tile.at(xu, yu, 1);
                let b = tile.at(xu, yu, 2);
                let oklab = linear_to_oklab(engine_color::oklab::LinRgb { r, g, b });

                // Add accumulated error
                let err_idx = (y * size + x) * 3;
                let adjusted_l = oklab.l + error_buf[err_idx];
                let adjusted_a = oklab.a + error_buf[err_idx + 1];
                let adjusted_b = oklab.b + error_buf[err_idx + 2];

                // Clamp to valid Oklab ranges
                let clamped = Oklab {
                    l: adjusted_l.clamp(0.0, 1.0),
                    a: adjusted_a.clamp(-0.5, 0.5),
                    b: adjusted_b.clamp(-0.5, 0.5),
                };

                // Find nearest palette color (O(1) LUT)
                let nearest_idx = lut.nearest_index(clamped) as usize;
                let nearest_oklab = palette_oklab[nearest_idx];
                let palette_color = &palette.colors[nearest_idx];

                // Compute error (difference between adjusted and quantized)
                let err_l = clamped.l - nearest_oklab.l;
                let err_a = clamped.a - nearest_oklab.a;
                let err_b = clamped.b - nearest_oklab.b;

                // Distribute error to neighbors
                Self::distribute_error(
                    &mut error_buf,
                    x,
                    y,
                    size,
                    err_l,
                    err_a,
                    err_b,
                    kernel,
                );

                // Write palette color (CRITICAL: always write exact palette entry)
                output.set(xu, yu, 0, palette_color.r);
                output.set(xu, yu, 1, palette_color.g);
                output.set(xu, yu, 2, palette_color.b);
                // Preserve alpha unmodified
                output.set(xu, yu, 3, tile.at(xu, yu, 3));
            }
        }

        Ok(output)
    }

    /// Distribute quantization error to neighboring pixels according to the kernel.
    ///
    /// Truncates at tile boundaries (no cross-tile error transfer).
    #[inline]
    fn distribute_error(
        error_buf: &mut [f32],
        x: usize,
        y: usize,
        size: usize,
        err_l: f32,
        err_a: f32,
        err_b: f32,
        kernel: DiffusionKernel,
    ) {
        Self::apply_kernel(error_buf, x, y, size, err_l, err_a, err_b, kernel.offsets());
    }

    /// Apply a set of (dx, dy, weight) offsets to distribute error into the buffer.
    /// Truncates at tile boundaries.
    #[inline]
    fn apply_kernel(
        error_buf: &mut [f32],
        x: usize,
        y: usize,
        size: usize,
        err_l: f32,
        err_a: f32,
        err_b: f32,
        offsets: &[(i32, i32, f32)],
    ) {
        for &(dx, dy, weight) in offsets {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;

            // Truncate at boundaries
            if nx >= 0 && nx < size as i32 && ny >= 0 && ny < size as i32 {
                let idx = (ny as usize * size + nx as usize) * 3;
                error_buf[idx] += err_l * weight;
                error_buf[idx + 1] += err_a * weight;
                error_buf[idx + 2] += err_b * weight;
            }
        }
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use engine_color::palette::LinearColor;
    use engine_color::palette_cache::PaletteKdCache;
    use engine_color::palette_lut::DEFAULT_LUT_SIZE;

    /// Helper: create a small palette with known colors.
    fn make_test_palette(colors: Vec<LinearColor>) -> Palette {
        Palette {
            id: 1,
            name: "Test".to_string(),
            colors,
            revision: 1,
        }
    }

    /// Helper: build a LUT from a palette (via KD at cell centers).
    fn build_lut(palette: &Palette) -> PaletteLut3D {
        let kd = PaletteKdCache::new();
        let tree = kd.get_or_build(palette).unwrap();
        PaletteLut3D::build(palette, DEFAULT_LUT_SIZE, &tree).unwrap()
    }

    /// Helper: create a tile filled with a single color.
    fn make_solid_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
        let mut tile = PixelTile::new();
        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                tile.set(x, y, 0, r);
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, b);
                tile.set(x, y, 3, a);
            }
        }
        tile
    }

    fn default_coord() -> TileCoord {
        TileCoord { level: 0, x: 0, y: 0 }
    }

    #[test]
    fn test_nearest_only_quantization() {
        // Palette: black and white
        let palette = make_test_palette(vec![
            LinearColor { r: 0.0, g: 0.0, b: 0.0 }, // black
            LinearColor { r: 1.0, g: 1.0, b: 1.0 }, // white
        ]);
        let lut = build_lut(&palette);

        // Input: mid-gray tile (closer to one or the other in Oklab)
        let tile = make_solid_tile(0.8, 0.8, 0.8, 0.5);

        let result = PaletteQuantizeFilter::apply(&tile, default_coord(), &palette, &lut, None)
            .unwrap();

        // Output should be white (0.8 linear is closer to white in Oklab)
        let out_r = result.at(10, 10, 0);
        let out_g = result.at(10, 10, 1);
        let out_b = result.at(10, 10, 2);
        assert_eq!(out_r, 1.0);
        assert_eq!(out_g, 1.0);
        assert_eq!(out_b, 1.0);

        // Alpha preserved
        assert_eq!(result.at(10, 10, 3), 0.5);
    }

    #[test]
    fn test_nearest_maps_to_closest_palette_color() {
        // Palette: red, green, blue
        let palette = make_test_palette(vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 }, // red
            LinearColor { r: 0.0, g: 1.0, b: 0.0 }, // green
            LinearColor { r: 0.0, g: 0.0, b: 1.0 }, // blue
        ]);
        let lut = build_lut(&palette);

        // Input: a strongly red tile
        let tile = make_solid_tile(0.9, 0.1, 0.05, 1.0);

        let result = PaletteQuantizeFilter::apply(&tile, default_coord(), &palette, &lut, None)
            .unwrap();

        // Should map to red
        assert_eq!(result.at(5, 5, 0), 1.0);
        assert_eq!(result.at(5, 5, 1), 0.0);
        assert_eq!(result.at(5, 5, 2), 0.0);
    }

    #[test]
    fn test_error_diffusion_floyd_steinberg() {
        // Palette: black and white
        let palette = make_test_palette(vec![
            LinearColor { r: 0.0, g: 0.0, b: 0.0 },
            LinearColor { r: 1.0, g: 1.0, b: 1.0 },
        ]);
        let lut = build_lut(&palette);

        // Input: mid-gray tile
        let tile = make_solid_tile(0.5, 0.5, 0.5, 1.0);

        let result = PaletteQuantizeFilter::apply(
            &tile,
            default_coord(),
            &palette,
            &lut,
            Some(DiffusionKernel::FloydSteinberg),
        )
        .unwrap();

        // With error diffusion on a uniform gray, we expect a mix of black and white
        // Count blacks and whites in a sample region
        let mut black_count = 0u32;
        let mut white_count = 0u32;
        for y in 10..50 {
            for x in 10..50 {
                let r = result.at(x, y, 0);
                if r == 0.0 {
                    black_count += 1;
                } else if r == 1.0 {
                    white_count += 1;
                } else {
                    panic!("Output pixel ({}, {}) has r={} which is neither black nor white", x, y, r);
                }
            }
        }
        // Both should be present (error diffusion creates a dither pattern)
        assert!(black_count > 0, "Expected some black pixels");
        assert!(white_count > 0, "Expected some white pixels");
    }

    #[test]
    fn test_empty_palette_error() {
        let palette = make_test_palette(vec![]);
        // Can't build a tree from empty palette, but we need to test the filter error path
        // The filter should reject empty palettes before using the tree
        let dummy_palette_for_tree = make_test_palette(vec![
            LinearColor { r: 0.5, g: 0.5, b: 0.5 },
        ]);
        let lut = build_lut(&dummy_palette_for_tree);

        let tile = make_solid_tile(0.5, 0.5, 0.5, 1.0);

        let result = PaletteQuantizeFilter::apply(&tile, default_coord(), &palette, &lut, None);
        assert!(result.is_err());
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error for empty palette"),
        };
        assert!(
            err.to_string().contains("non-empty palette"),
            "Expected empty palette error, got: {}",
            err
        );
    }

    #[test]
    fn test_palette_membership_invariant() {
        // Every output pixel must exactly match a palette entry
        let palette = make_test_palette(vec![
            LinearColor { r: 0.2, g: 0.1, b: 0.3 },
            LinearColor { r: 0.7, g: 0.8, b: 0.4 },
            LinearColor { r: 0.0, g: 0.5, b: 1.0 },
        ]);
        let lut = build_lut(&palette);

        // Create a tile with varied colors
        let mut tile = PixelTile::new();
        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                let fx = x as f32 / FULL_SIZE as f32;
                let fy = y as f32 / FULL_SIZE as f32;
                tile.set(x, y, 0, fx);
                tile.set(x, y, 1, fy);
                tile.set(x, y, 2, (fx + fy) * 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }

        // Test with nearest-only
        let result = PaletteQuantizeFilter::apply(&tile, default_coord(), &palette, &lut, None)
            .unwrap();

        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                let out_r = result.at(x, y, 0);
                let out_g = result.at(x, y, 1);
                let out_b = result.at(x, y, 2);

                let matches_any = palette.colors.iter().any(|c| {
                    c.r == out_r && c.g == out_g && c.b == out_b
                });
                assert!(
                    matches_any,
                    "Pixel ({}, {}) = ({}, {}, {}) does not match any palette entry",
                    x, y, out_r, out_g, out_b
                );
            }
        }
    }

    #[test]
    fn test_palette_membership_with_diffusion() {
        // Same invariant must hold even with error diffusion
        let palette = make_test_palette(vec![
            LinearColor { r: 0.0, g: 0.0, b: 0.0 },
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            LinearColor { r: 0.0, g: 0.0, b: 1.0 },
            LinearColor { r: 1.0, g: 1.0, b: 1.0 },
        ]);
        let lut = build_lut(&palette);

        let tile = make_solid_tile(0.4, 0.6, 0.3, 0.75);

        let result = PaletteQuantizeFilter::apply(
            &tile,
            default_coord(),
            &palette,
            &lut,
            Some(DiffusionKernel::Atkinson),
        )
        .unwrap();

        // Check a sample of pixels for membership
        for y in (0..FULL_SIZE).step_by(5) {
            for x in (0..FULL_SIZE).step_by(5) {
                let out_r = result.at(x, y, 0);
                let out_g = result.at(x, y, 1);
                let out_b = result.at(x, y, 2);

                let matches_any = palette.colors.iter().any(|c| {
                    c.r == out_r && c.g == out_g && c.b == out_b
                });
                assert!(
                    matches_any,
                    "Pixel ({}, {}) = ({}, {}, {}) does not match any palette entry",
                    x, y, out_r, out_g, out_b
                );
            }
        }
    }

    #[test]
    fn test_alpha_preservation() {
        let palette = make_test_palette(vec![
            LinearColor { r: 0.0, g: 0.0, b: 0.0 },
            LinearColor { r: 1.0, g: 1.0, b: 1.0 },
        ]);
        let lut = build_lut(&palette);

        // Create tile with varying alpha
        let mut tile = PixelTile::new();
        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                let alpha = (x as f32 + y as f32) / (2.0 * FULL_SIZE as f32);
                tile.set(x, y, 0, 0.5);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, alpha);
            }
        }

        let result = PaletteQuantizeFilter::apply(
            &tile,
            default_coord(),
            &palette,
            &lut,
            Some(DiffusionKernel::FloydSteinberg),
        )
        .unwrap();

        // Verify alpha is preserved exactly
        for y in 0..FULL_SIZE {
            for x in 0..FULL_SIZE {
                assert_eq!(
                    result.at(x, y, 3),
                    tile.at(x, y, 3),
                    "Alpha mismatch at ({}, {})",
                    x, y
                );
            }
        }
    }
}
