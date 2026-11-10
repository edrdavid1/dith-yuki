//! Strip vs contiguous invariance for `glitch_dither`, including heights that
//! cross tile boundaries (global line index must be `ty·256 + local`).

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_ed1d_tile_row_strip;
use engine_project::filters::ed1d::{
    ed1d_image, Axis, Ed1dCfg, SrcXf, UniformLevelsQuantizer, WeightFn,
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

#[test]
fn strip_matches_contiguous_across_tile_rows() {
    // Heights 256 and 512 (tile multiples) via strip vs kernel.
    for height in [256u32, 512] {
        let width = 512u32;
        let src = fill_gradient(width, height);
        let n_tx = width / TILE_SIZE;
        let n_ty = height / TILE_SIZE;
        let doc = Document::new(DocumentId::new(1), width, height);
        let params = DitherParamsV2 {
            mode: DitherModeV2::GlitchDither,
            levels: 4,
            glitch_intensity: 12,
            glitch_seed: 42,
            color_mode: DitherColorMode::Rgb,
            ..DitherParamsV2::default()
        };
        let pc = PaletteKdCache::new();
        let lc = PaletteLutCache::new();

        let mut outputs_all: Vec<(TileCoord, PixelTile)> = Vec::new();
        for ty in 0..n_ty {
            let owned: Vec<(TileCoord, PixelTile)> = (0..n_tx)
                .map(|tx| {
                    (
                        TileCoord {
                            level: 0,
                            x: tx,
                            y: ty,
                        },
                        tile_from_buffer(&src, width, height, tx, ty),
                    )
                })
                .collect();
            let refs: Vec<(TileCoord, &PixelTile)> = owned.iter().map(|(c, t)| (*c, t)).collect();
            let mut outputs: Vec<PixelTile> =
                (0..n_tx as usize).map(|_| PixelTile::new()).collect();
            apply_ed1d_tile_row_strip(&refs, &params, &pc, &lc, &doc, &mut outputs).unwrap();
            for (i, (c, _)) in owned.into_iter().enumerate() {
                outputs_all.push((c, std::mem::replace(&mut outputs[i], PixelTile::new())));
            }
        }

        let cfg = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            src_transform: Some(SrcXf::Glitch {
                intensity: 12,
                seed: 42,
            }),
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let reference = ed1d_image(&src, width as usize, height as usize, &cfg, &mut q);

        for ty in 0..n_ty {
            for tx in 0..n_tx {
                let tile = outputs_all
                    .iter()
                    .find(|(c, _)| c.x == tx && c.y == ty)
                    .map(|(_, t)| t)
                    .unwrap();
                for ly in 0..TILE_SIZE {
                    for lx in 0..TILE_SIZE {
                        let gx = tx * TILE_SIZE + lx;
                        let gy = ty * TILE_SIZE + ly;
                        let i = ((gy * width + gx) * 4) as usize;
                        for c in 0..3u32 {
                            let got = tile.at(lx + HALO, ly + HALO, c);
                            let exp = reference[i + c as usize];
                            assert!(
                                (got - exp).abs() < 1e-5,
                                "mismatch h={height} at ({gx},{gy}) c={c}: {got} vs {exp}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn kernel_global_line_odd_sizes() {
    // Contiguous path for widths/heights that the strip grid cannot express.
    let cfg = Ed1dCfg {
        axis: Axis::Horizontal,
        carry: 1.0,
        weight: WeightFn::Const,
        ema: None,
        src_transform: Some(SrcXf::Glitch {
            intensity: 8,
            seed: 42,
        }),
        ..Ed1dCfg::default()
    };
    for &(w, h) in &[
        (256u32, 255),
        (257, 256),
        (512, 513),
        (1000, 1000),
        (1, 8),
        (2, 2),
        (7, 9),
        (257, 255),
    ] {
        let src = fill_gradient(w, h);
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let out = ed1d_image(&src, w as usize, h as usize, &cfg, &mut q);
        assert_eq!(out.len(), (w * h * 4) as usize);
    }
}

#[test]
fn crossing_tile_boundary_uses_global_line() {
    // Two strips at ty=0 and ty=1 must match one contiguous 512-tall buffer:
    // line 256 in strip ty=1 equals global line 256, not local 0.
    let width = 256u32;
    let height = 512u32;
    let src = fill_gradient(width, height);
    let cfg = Ed1dCfg {
        axis: Axis::Horizontal,
        carry: 1.0,
        weight: WeightFn::Const,
        ema: None,
        src_transform: Some(SrcXf::Glitch {
            intensity: 20,
            seed: 7,
        }),
        ..Ed1dCfg::default()
    };
    let mut q = UniformLevelsQuantizer { levels: 4.0 };
    let full = ed1d_image(&src, width as usize, height as usize, &cfg, &mut q);

    let doc = Document::new(DocumentId::new(1), width, height);
    let params = DitherParamsV2 {
        mode: DitherModeV2::GlitchDither,
        levels: 4,
        glitch_intensity: 20,
        glitch_seed: 7,
        ..DitherParamsV2::default()
    };
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();
    for ty in 0..2u32 {
        let tile = tile_from_buffer(&src, width, height, 0, ty);
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: ty,
        };
        let mut out = [PixelTile::new()];
        apply_ed1d_tile_row_strip(&[(coord, &tile)], &params, &pc, &lc, &doc, &mut out).unwrap();
        for ly in 0..TILE_SIZE {
            for lx in 0..TILE_SIZE {
                let gy = ty * TILE_SIZE + ly;
                let i = ((gy * width + lx) * 4) as usize;
                for c in 0..3u32 {
                    let got = out[0].at(lx + HALO, ly + HALO, c);
                    let exp = full[i + c as usize];
                    assert!(
                        (got - exp).abs() < 1e-5,
                        "global line bug at ty={ty} ly={ly}: {got} vs {exp}"
                    );
                }
            }
        }
    }
}
