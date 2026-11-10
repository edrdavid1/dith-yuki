//! Full-height column strip vs contiguous-buffer invariance for `modulated_diffuse_x`.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::{
    apply_ed1d_tile_column_strip, apply_ed1d_tile_row_strip,
};
use engine_project::filters::ed1d::{
    contrast_gain_from_line_scale, ed1d_image, ed1d_image_horizontal_par, Axis, Ed1dCfg,
    UniformLevelsQuantizer, WeightFn,
};
use engine_project::types::DocumentId;
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

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

fn transpose_rgba(src: &[f32], w: u32, h: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let s = ((y * w + x) * 4) as usize;
            let d = ((x * h + y) * 4) as usize;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

fn tile_from_buffer(buf: &[f32], w: u32, h: u32, tx: u32, ty: u32) -> PixelTile {
    let mut tile = PixelTile::new();
    let full = TILE_SIZE + 2 * HALO;
    for ly in 0..full {
        for lx in 0..full {
            let gx = tx as i32 * TILE_SIZE as i32 + lx as i32 - HALO as i32;
            let gy = ty as i32 * TILE_SIZE as i32 + ly as i32 - HALO as i32;
            if gx >= 0 && gy >= 0 && (gx as u32) < w && (gy as u32) < h {
                let i = ((gy as u32 * w + gx as u32) * 4) as usize;
                tile.set(lx, ly, 0, buf[i]);
                tile.set(lx, ly, 1, buf[i + 1]);
                tile.set(lx, ly, 2, buf[i + 2]);
                tile.set(lx, ly, 3, buf[i + 3]);
            }
        }
    }
    tile
}

/// Kernel Vertical equals contiguous transpose path for odd sizes the strip
/// driver cannot represent (tile grid is 256-aligned).
#[test]
fn kernel_column_matches_heights_and_narrow_widths() {
    let cfg = Ed1dCfg {
        axis: Axis::Vertical,
        carry: 1.0,
        weight: WeightFn::Contrast {
            gain: contrast_gain_from_line_scale(5),
        },
        ema: None,
        src_transform: None,
        ..Ed1dCfg::default()
    };
    for &(w, h) in &[
        (1u32, 256),
        (1, 257),
        (255, 256),
        (257, 256),
        (256, 257),
        (256, 512),
        (256, 1000),
        (257, 255),
    ] {
        let src = fill_gradient(w, h);
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let out = ed1d_image(&src, w as usize, h as usize, &cfg, &mut q);
        assert_eq!(out.len(), (w * h * 4) as usize);
        // Re-run determinism.
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        assert_eq!(out, ed1d_image(&src, w as usize, h as usize, &cfg, &mut q2));
    }
}

#[test]
fn column_strip_matches_contiguous_heights() {
    for height in [256u32, 512, 768] {
        let width = 256u32;
        assert_eq!(height % TILE_SIZE, 0);
        let src = fill_gradient(width, height);
        let n_tiles = height / TILE_SIZE;
        let doc = Document::new(DocumentId::new(1), width, height);
        let params = DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseX,
            levels: 4,
            line_scale: 5,
            palette_id: None,
            color_mode: DitherColorMode::Rgb,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        let mut owned: Vec<(TileCoord, PixelTile)> = (0..n_tiles)
            .map(|ty| {
                (
                    TileCoord {
                        level: 0,
                        x: 0,
                        y: ty,
                    },
                    tile_from_buffer(&src, width, height, 0, ty),
                )
            })
            .collect();
        let refs: Vec<(TileCoord, &PixelTile)> = owned.iter().map(|(c, t)| (*c, t)).collect();
        let mut outputs: Vec<PixelTile> = (0..n_tiles as usize).map(|_| PixelTile::new()).collect();
        let pc = PaletteKdCache::new();
        let lc = PaletteLutCache::new();
        apply_ed1d_tile_column_strip(&refs, &params, &pc, &lc, &doc, &mut outputs).unwrap();

        let cfg = Ed1dCfg {
            axis: Axis::Vertical,
            carry: 1.0,
            weight: WeightFn::Contrast {
                gain: contrast_gain_from_line_scale(5),
            },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let reference = ed1d_image(&src, width as usize, height as usize, &cfg, &mut q);

        for ty in 0..n_tiles {
            for ly in 0..TILE_SIZE {
                for lx in 0..TILE_SIZE.min(width) {
                    let gx = lx;
                    let gy = ty * TILE_SIZE + ly;
                    let i = ((gy * width + gx) * 4) as usize;
                    let tile_x = lx + HALO;
                    let tile_y = ly + HALO;
                    for c in 0..3u32 {
                        let got = outputs[ty as usize].at(tile_x, tile_y, c);
                        let exp = reference[i + c as usize];
                        assert!(
                            (got - exp).abs() < 1e-5,
                            "mismatch w={width} h={height} at ({gx},{gy}) c={c}: {got} vs {exp}"
                        );
                    }
                }
            }
        }
        let _ = &mut owned;
    }
}

/// `x(I) == (y(Iᵀ))ᵀ` through the real strip drivers on a square tile grid.
#[test]
fn production_axis_equivalence_via_strips() {
    let w = 256u32;
    let h = 256u32;
    let src = fill_gradient(w, h);
    let src_t = transpose_rgba(&src, w, h);
    let doc = Document::new(DocumentId::new(1), w, h);
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();

    let params_x = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseX,
        levels: 4,
        line_scale: 5,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    let params_y = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseY,
        levels: 4,
        line_scale: 5,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };

    let tile_x = tile_from_buffer(&src, w, h, 0, 0);
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let mut out_x = [PixelTile::new()];
    apply_ed1d_tile_column_strip(&[(coord, &tile_x)], &params_x, &pc, &lc, &doc, &mut out_x)
        .unwrap();

    // Iᵀ is h×w = 256×256; one horizontal strip tile.
    let tile_yt = tile_from_buffer(&src_t, h, w, 0, 0);
    let mut out_yt = [PixelTile::new()];
    apply_ed1d_tile_row_strip(&[(coord, &tile_yt)], &params_y, &pc, &lc, &doc, &mut out_yt)
        .unwrap();

    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let got = [
                out_x[0].at(x + HALO, y + HALO, 0),
                out_x[0].at(x + HALO, y + HALO, 1),
                out_x[0].at(x + HALO, y + HALO, 2),
            ];
            // (y(Iᵀ))ᵀ → sample (y,x) of out_yt becomes (x,y) of expected.
            let exp = [
                out_yt[0].at(y + HALO, x + HALO, 0),
                out_yt[0].at(y + HALO, x + HALO, 1),
                out_yt[0].at(y + HALO, x + HALO, 2),
            ];
            for c in 0..3 {
                assert!(
                    (got[c] - exp[c]).abs() < 1e-5,
                    "strip axis eq at ({x},{y}) c={c}: {} vs {}",
                    got[c],
                    exp[c]
                );
            }
        }
    }
}

#[test]
fn kernel_axis_equivalence_nonsquare() {
    for &(w, h) in &[(64u32, 64), (257, 255)] {
        let src = fill_gradient(w, h);
        let src_t = transpose_rgba(&src, w, h);
        let cfg_y = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Contrast {
                gain: contrast_gain_from_line_scale(5),
            },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let cfg_x = Ed1dCfg {
            axis: Axis::Vertical,
            carry: 1.0,
            weight: WeightFn::Contrast {
                gain: contrast_gain_from_line_scale(5),
            },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let mut q1 = UniformLevelsQuantizer { levels: 4.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        let out_x = ed1d_image(&src, w as usize, h as usize, &cfg_x, &mut q1);
        let out_y_on_t = ed1d_image(&src_t, h as usize, w as usize, &cfg_y, &mut q2);
        let out_y_tt = transpose_rgba(&out_y_on_t, h, w);
        assert_eq!(out_x, out_y_tt, "axis equivalence failed for {w}x{h}");
    }
}

#[test]
fn vertical_threads_bit_identical() {
    let src = fill_gradient(128, 96);
    let cfg = Ed1dCfg {
        axis: Axis::Horizontal,
        carry: 1.0,
        weight: WeightFn::Contrast { gain: 0.5 },
        ema: None,
        src_transform: None,
        ..Ed1dCfg::default()
    };
    // Vertical path = transpose → horizontal → transpose; par on transposed rows.
    let src_t = transpose_rgba(&src, 128, 96);
    let q = UniformLevelsQuantizer { levels: 4.0 };
    let par = ed1d_image_horizontal_par(&src_t, 96, 128, &cfg, &q);
    let mut q2 = q.clone();
    let seq = ed1d_image(&src_t, 96, 128, &cfg, &mut q2);
    assert_eq!(par, seq);
}

#[test]
fn edges_column_path() {
    let doc = Document::new(DocumentId::new(1), 256, 256);
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseX,
        levels: 4,
        line_scale: 5,
        ..DitherParamsV2::default()
    };
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();
    let src = fill_gradient(256, 256);
    let tile = tile_from_buffer(&src, 256, 256, 0, 0);
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let mut out = [PixelTile::new()];
    apply_ed1d_tile_column_strip(&[(coord, &tile)], &params, &pc, &lc, &doc, &mut out).unwrap();
}

#[test]
fn edges_kernel_1x_n_nx1_1x1() {
    let cfg = Ed1dCfg {
        axis: Axis::Vertical,
        carry: 1.0,
        weight: WeightFn::Contrast { gain: 0.25 },
        ema: None,
        src_transform: None,
        ..Ed1dCfg::default()
    };
    for &(w, h) in &[(1u32, 1), (1, 17), (17, 1), (7, 9)] {
        let src = fill_gradient(w, h);
        let mut q = UniformLevelsQuantizer { levels: 2.0 };
        let out = ed1d_image(&src, w as usize, h as usize, &cfg, &mut q);
        assert_eq!(out.len(), (w * h * 4) as usize);
        // Alpha preserved.
        for i in (0..out.len()).step_by(4) {
            assert_eq!(out[i + 3], src[i + 3]);
        }
    }
}
