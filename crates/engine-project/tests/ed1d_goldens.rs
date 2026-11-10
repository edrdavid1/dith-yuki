//! Golden hashes for `modulated_diffuse_y` (Strict + Simple × line_scale).
//!
//! Record: `ED1D_GOLDEN_RECORD=1 cargo test -p engine-project --test ed1d_goldens -- --nocapture`

use std::collections::BTreeMap;

use blake3::Hasher;
use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};
use engine_project::filters::dither_diffusion::{
    apply_ed1d_tile_column_strip, apply_ed1d_tile_row_strip,
};
use engine_project::types::DocumentId;
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const W: u32 = 512;
const H: u32 = 512;

/// Captured after strip path + parallel rows landed. Do not edit unless intentional.
const EXPECTED: &[(&str, &str)] = &[
    (
        "modulated_diffuse_x/ls1/simple",
        "795a906c57ad779ac3c04df9a6be2500a561672496624857ae405c877697b887",
    ),
    (
        "modulated_diffuse_x/ls1/strict",
        "cd6f63106ed2106fabfdd193bae9d72e88a4e6855e08f353969f572a80be6329",
    ),
    (
        "modulated_diffuse_x/ls20/simple",
        "d43311d1174e0a35fe6aacff8e4cdbc8e438cc89a92cb0bdaaf77e4f27a5a27f",
    ),
    (
        "modulated_diffuse_x/ls20/strict",
        "39a24ebe22d8e4b9a62eb6953afdca1cf936f507e9ef1eeca7787060aefd67e0",
    ),
    (
        "modulated_diffuse_x/ls5/simple",
        "80e719886e392ab0a821a93b80e5e0ac4a9a6b56dd9f99788bbba91129f68ad3",
    ),
    (
        "modulated_diffuse_x/ls5/strict",
        "d628f02aa07ada0e66d2a9182efee047c706685227f0a297ed4a3ed3bb559388",
    ),
    (
        "modulated_diffuse_y/ls1/simple",
        "b39ddca7724161528ca35b5fa3561d4b705db022dde8ec582698ca0f09833c41",
    ),
    (
        "modulated_diffuse_y/ls1/strict",
        "34cb3a11943cf46af102176723968ac73c0b48068b6fbbbc35585acaa76be61d",
    ),
    (
        "modulated_diffuse_y/ls20/simple",
        "b0e701232212201f8f4b52285354b9d3c93b6e441ee98016e1ed6f62fa28e747",
    ),
    (
        "modulated_diffuse_y/ls20/strict",
        "094cef15799192f9771f3932731f78ed446e4c843ed3d35390441db4ea843eb9",
    ),
    (
        "modulated_diffuse_y/ls5/simple",
        "8e1644cf1f2de33cc66090ffcf63629e8d8206166531eb21f1e9bf5329d8ab99",
    ),
    (
        "modulated_diffuse_y/ls5/strict",
        "ffdebc5f570b3a1445121d53435606ec74331826b966c7c3a2ef250a801bfdda",
    ),
];

fn four_color_palette() -> Vec<LinearColor> {
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

fn fill_gradient_tile(tx: u32, ty: u32) -> PixelTile {
    let full = TILE_SIZE + 2 * HALO;
    let mut t = PixelTile::new();
    for y in 0..full {
        for x in 0..full {
            let gx = (tx * TILE_SIZE + x.saturating_sub(HALO)) as f32;
            let gy = (ty * TILE_SIZE + y.saturating_sub(HALO)) as f32;
            let u = (gx / W as f32).clamp(0.0, 1.0);
            let v = (gy / H as f32).clamp(0.0, 1.0);
            let r = (u * 0.85 + (v * 12.0).sin().abs() * 0.15).clamp(0.0, 1.0);
            let g = (v * 0.85 + (u * 9.0).cos().abs() * 0.15).clamp(0.0, 1.0);
            let b = ((u + v) * 0.5).clamp(0.0, 1.0);
            t.set(x, y, 0, r);
            t.set(x, y, 1, g);
            t.set(x, y, 2, b);
            t.set(x, y, 3, 1.0);
        }
    }
    t
}

fn hash_core_rgb(tiles: &[(TileCoord, PixelTile)], cols: u32, rows: u32) -> String {
    let mut h = Hasher::new();
    for ty in 0..rows {
        for local_y in 0..TILE_SIZE {
            for tx in 0..cols {
                let tile = &tiles
                    .iter()
                    .find(|(c, _)| c.x == tx && c.y == ty)
                    .expect("tile")
                    .1;
                let y = local_y + HALO;
                for local_x in 0..TILE_SIZE {
                    let x = local_x + HALO;
                    for c in 0..3u32 {
                        h.update(&tile.at(x, y, c).to_le_bytes());
                    }
                }
            }
        }
    }
    h.finalize().to_hex().to_string()
}

fn run_case_y(line_scale: u8, pdm: PaletteDitherMode) -> String {
    let mut doc = Document::new(DocumentId::new(1), W, H);
    let pid = doc.add_palette("ed1d4".into(), four_color_palette());
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseY,
        levels: 4,
        line_scale,
        palette_id: Some(pid),
        palette_dither_mode: pdm,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    let cols = W / TILE_SIZE;
    let rows = H / TILE_SIZE;
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();

    let mut out_store: Vec<(TileCoord, PixelTile)> = Vec::new();
    for ty in 0..rows {
        let owned: Vec<(TileCoord, PixelTile)> = (0..cols)
            .map(|tx| {
                (
                    TileCoord {
                        level: 0,
                        x: tx,
                        y: ty,
                    },
                    fill_gradient_tile(tx, ty),
                )
            })
            .collect();
        let refs: Vec<(TileCoord, &PixelTile)> = owned.iter().map(|(c, t)| (*c, t)).collect();
        let mut outputs: Vec<PixelTile> = (0..cols as usize).map(|_| PixelTile::new()).collect();
        apply_ed1d_tile_row_strip(&refs, &params, &pc, &lc, &doc, &mut outputs).unwrap();
        for (i, (c, _)) in owned.into_iter().enumerate() {
            out_store.push((c, std::mem::replace(&mut outputs[i], PixelTile::new())));
        }
    }
    hash_core_rgb(&out_store, cols, rows)
}

fn run_case_x(line_scale: u8, pdm: PaletteDitherMode) -> String {
    let mut doc = Document::new(DocumentId::new(1), W, H);
    let pid = doc.add_palette("ed1d4x".into(), four_color_palette());
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseX,
        levels: 4,
        line_scale,
        palette_id: Some(pid),
        palette_dither_mode: pdm,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    let cols = W / TILE_SIZE;
    let rows = H / TILE_SIZE;
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();

    let mut out_store: Vec<(TileCoord, PixelTile)> = Vec::new();
    for tx in 0..cols {
        let owned: Vec<(TileCoord, PixelTile)> = (0..rows)
            .map(|ty| {
                (
                    TileCoord {
                        level: 0,
                        x: tx,
                        y: ty,
                    },
                    fill_gradient_tile(tx, ty),
                )
            })
            .collect();
        let refs: Vec<(TileCoord, &PixelTile)> = owned.iter().map(|(c, t)| (*c, t)).collect();
        let mut outputs: Vec<PixelTile> = (0..rows as usize).map(|_| PixelTile::new()).collect();
        apply_ed1d_tile_column_strip(&refs, &params, &pc, &lc, &doc, &mut outputs).unwrap();
        for (i, (c, _)) in owned.into_iter().enumerate() {
            out_store.push((c, std::mem::replace(&mut outputs[i], PixelTile::new())));
        }
    }
    hash_core_rgb(&out_store, cols, rows)
}

#[test]
fn ed1d_goldens() {
    let record = std::env::var("ED1D_GOLDEN_RECORD").ok().as_deref() == Some("1");
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for ls in [1u8, 5, 20] {
        for (tag, pdm) in [
            ("strict", PaletteDitherMode::Strict),
            ("simple", PaletteDitherMode::Simple),
        ] {
            let key_y = format!("modulated_diffuse_y/ls{ls}/{tag}");
            let digest_y = run_case_y(ls, pdm.clone());
            println!("{key_y} = {digest_y}");
            got.insert(key_y, digest_y);

            let key_x = format!("modulated_diffuse_x/ls{ls}/{tag}");
            let digest_x = run_case_x(ls, pdm);
            println!("{key_x} = {digest_x}");
            got.insert(key_x, digest_x);
        }
    }

    if record {
        println!("--- paste into EXPECTED ---");
        for (k, v) in &got {
            println!("    (\"{k}\", \"{v}\"),");
        }
        return;
    }

    if EXPECTED.is_empty() {
        panic!("EXPECTED is empty — run ED1D_GOLDEN_RECORD=1 and paste tuples into EXPECTED");
    }
    let expected: BTreeMap<_, _> = EXPECTED.iter().copied().collect();
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "case set changed"
    );
    for (k, exp) in &expected {
        let g = got.get(*k).unwrap();
        assert_eq!(g, exp, "ed1d golden changed for {k}");
    }
}
