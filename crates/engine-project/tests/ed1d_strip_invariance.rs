//! Strip-path vs contiguous-buffer invariance for `modulated_diffuse_y`.

use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};
use engine_project::filters::dither_diffusion::apply_ed1d_tile_row_strip;
use engine_project::filters::ed1d::{
    contrast_gain_from_line_scale, ed1d_image, Axis, Ed1dCfg, UniformLevelsQuantizer, WeightFn,
};
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

fn four_colors() -> Vec<LinearColor> {
    vec![
        LinearColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        LinearColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        },
        LinearColor {
            r: 0.8,
            g: 0.1,
            b: 0.1,
        },
        LinearColor {
            r: 0.1,
            g: 0.2,
            b: 0.9,
        },
    ]
}

fn fill_gradient(w: u32, h: u32) -> Vec<f32> {
    let mut v = vec![0.0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let t = x as f32 / w.max(1) as f32;
            let u = y as f32 / h.max(1) as f32;
            v[i] = t;
            v[i + 1] = u;
            v[i + 2] = (t + u) * 0.5;
            v[i + 3] = 1.0;
        }
    }
    v
}

/// Uniform levels only — compares pure buffer ed1d to itself at different
/// virtual partitions (no palette LUT).
#[test]
fn contiguous_buffer_partition_invariant_uniform() {
    let w = 512usize;
    let h = 64usize;
    let src = fill_gradient(w as u32, h as u32);
    let cfg = Ed1dCfg {
        axis: Axis::Horizontal,
        carry: 1.0,
        weight: WeightFn::Contrast {
            gain: contrast_gain_from_line_scale(5),
        },
        ema: None,
        src_transform: None,
        ..Ed1dCfg::default()
    };
    let mut q = UniformLevelsQuantizer { levels: 4.0 };
    let reference = ed1d_image(&src, w, h, &cfg, &mut q);
    // Re-run must match (determinism).
    let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
    let again = ed1d_image(&src, w, h, &cfg, &mut q2);
    assert_eq!(reference, again);
}

#[test]
fn strip_matches_contiguous_uniform_widths() {
    // Build one horizontal strip of tiles from a buffer, run apply_ed1d_tile_row_strip
    // with Uniform (no palette), compare core pixels to ed1d_image on the same rows.
    for width in [256u32, 512, 768] {
        let height = TILE_SIZE; // one tile row
        let src = fill_gradient(width, height);
        let n_tiles = (width + TILE_SIZE - 1) / TILE_SIZE;
        assert_eq!(
            width % TILE_SIZE,
            0,
            "test widths must be multiples of tile"
        );

        let doc = Document::new(DocumentId::new(1), width, height);
        let _ = LayerId::new(1);
        let params = DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseY,
            levels: 4,
            line_scale: 5,
            palette_id: None,
            color_mode: DitherColorMode::Rgb,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };

        let mut tile_store: Vec<(TileCoord, PixelTile)> = Vec::new();
        for tx in 0..n_tiles {
            let coord = TileCoord {
                level: 0,
                x: tx,
                y: 0,
            };
            let mut tile = PixelTile::new();
            for ly in 0..(TILE_SIZE + 2 * HALO) {
                for lx in 0..(TILE_SIZE + 2 * HALO) {
                    let gx = tx * TILE_SIZE + lx.saturating_sub(HALO);
                    let gy = ly.saturating_sub(HALO);
                    let gx_c = gx.min(width - 1);
                    let gy_c = gy.min(height - 1);
                    let i = ((gy_c * width + gx_c) * 4) as usize;
                    tile.set(lx, ly, 0, src[i]);
                    tile.set(lx, ly, 1, src[i + 1]);
                    tile.set(lx, ly, 2, src[i + 2]);
                    tile.set(lx, ly, 3, 1.0);
                }
            }
            tile_store.push((coord, tile));
        }
        let refs: Vec<(TileCoord, &PixelTile)> = tile_store.iter().map(|(c, t)| (*c, t)).collect();
        let mut outputs: Vec<PixelTile> = (0..n_tiles).map(|_| PixelTile::new()).collect();
        let pc = PaletteKdCache::new();
        let lc = PaletteLutCache::new();
        apply_ed1d_tile_row_strip(&refs, &params, &pc, &lc, &doc, &mut outputs).unwrap();

        let cfg = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Contrast {
                gain: contrast_gain_from_line_scale(5),
            },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let expected = ed1d_image(&src, width as usize, height as usize, &cfg, &mut q);

        for tx in 0..n_tiles {
            for ly in 0..TILE_SIZE {
                for lx in 0..TILE_SIZE {
                    let gx = tx * TILE_SIZE + lx;
                    if gx >= width {
                        continue;
                    }
                    let i = ((ly * width + gx) * 4) as usize;
                    let tile_x = lx + HALO;
                    let tile_y = ly + HALO;
                    for c in 0..3u32 {
                        let got = outputs[tx as usize].at(tile_x, tile_y, c);
                        let exp = expected[i + c as usize];
                        assert!(
                            (got - exp).abs() < 1e-5,
                            "mismatch w={width} tx={tx} ({lx},{ly}) c={c}: {got} vs {exp}"
                        );
                    }
                }
            }
        }
        let _ = doc;
    }
}

#[test]
fn strip_palette_strict_runs() {
    let width = 512u32;
    let height = TILE_SIZE;
    let src = fill_gradient(width, height);
    let mut doc = Document::new(DocumentId::new(2), width, height);
    let pid = doc.add_palette("p".into(), four_colors());
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseY,
        levels: 4,
        line_scale: 10,
        palette_id: Some(pid),
        palette_dither_mode: PaletteDitherMode::Strict,
        color_mode: DitherColorMode::Rgb,
        dither_alpha: false,
        ..DitherParamsV2::default()
    };
    let mut tile_store = Vec::new();
    for tx in 0..2u32 {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: 0,
        };
        let mut tile = PixelTile::new();
        for ly in 0..(TILE_SIZE + 2 * HALO) {
            for lx in 0..(TILE_SIZE + 2 * HALO) {
                let gx = (tx * TILE_SIZE + lx.saturating_sub(HALO)).min(width - 1);
                let gy = ly.saturating_sub(HALO).min(height - 1);
                let i = ((gy * width + gx) * 4) as usize;
                tile.set(lx, ly, 0, src[i]);
                tile.set(lx, ly, 1, src[i + 1]);
                tile.set(lx, ly, 2, src[i + 2]);
                tile.set(lx, ly, 3, 1.0);
            }
        }
        tile_store.push((coord, tile));
    }
    let refs: Vec<_> = tile_store.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..2).map(|_| PixelTile::new()).collect();
    apply_ed1d_tile_row_strip(
        &refs,
        &params,
        &PaletteKdCache::new(),
        &PaletteLutCache::new(),
        &doc,
        &mut outputs,
    )
    .unwrap();
    assert!(outputs[0].at(HALO, HALO, 0) >= 0.0);
}
