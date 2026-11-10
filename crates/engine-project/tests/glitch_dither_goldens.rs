//! Golden hashes for `glitch_dither` (Strict + Simple × intensity).
//!
//! Record: `GLITCH_DITHER_GOLDEN_RECORD=1 cargo test -p engine-project --test glitch_dither_goldens -- --nocapture`

use std::collections::BTreeMap;

use blake3::Hasher;
use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};
use engine_project::filters::dither_diffusion::apply_ed1d_tile_row_strip;
use engine_project::types::DocumentId;
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const W: u32 = 512;
const H: u32 = 512;

/// Captured after strip path + global line index. Do not edit unless intentional.
const EXPECTED: &[(&str, &str)] = &[
    (
        "glitch_dither/i1/simple",
        "9cc0f6dd890b42c8668c575f9556473233e9440373a7f7173deca099fddea52f",
    ),
    (
        "glitch_dither/i1/strict",
        "69335700dec04db35bd5843636c3ac2b1124e23b8642350ac2fb4ddf9627d9c0",
    ),
    (
        "glitch_dither/i12/simple",
        "0a9560eede51489b29efa179aa43503f032899d1ecef36b6b59b74e247c89140",
    ),
    (
        "glitch_dither/i12/strict",
        "052bbbd4a11ab9f35abb2d6b0a280580948ccf4a2f16a588918d51b352c4f40c",
    ),
    (
        "glitch_dither/i20/simple",
        "c5b0fca6bfa5fae232fe620b1cc42ca384cade19642f95161c5c682a4e75a684",
    ),
    (
        "glitch_dither/i20/strict",
        "6f4a074c998da88d470a0f53299d30780aff27f4d3df0af17b31adb0f1057281",
    ),
    (
        "glitch_dither/i5/simple",
        "65c234c9a4f811b921db821da37c5a37e48fb27b77fff6d32aef0abae65aec5b",
    ),
    (
        "glitch_dither/i5/strict",
        "522f9eb9cc33535b1aed24178a8b12f660375cf01ec69c7122ed1b87f785d6d8",
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
    let mut tile = PixelTile::new();
    for y in 0..full {
        for x in 0..full {
            let gx = tx as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
            let gy = ty as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
            let t = ((gx + gy) as f32 * 0.07).sin() * 0.5 + 0.5;
            tile.set(x, y, 0, t);
            tile.set(x, y, 1, t * 0.85);
            tile.set(x, y, 2, 1.0 - t);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn hash_core_rgb(tiles: &[(TileCoord, PixelTile)], cols: u32, rows: u32) -> String {
    let mut hasher = Hasher::new();
    for ty in 0..rows {
        for tx in 0..cols {
            let tile = tiles
                .iter()
                .find(|(c, _)| c.x == tx && c.y == ty)
                .map(|(_, t)| t)
                .expect("tile");
            for ly in 0..TILE_SIZE {
                for lx in 0..TILE_SIZE {
                    for c in 0..3u32 {
                        hasher.update(&tile.at(lx + HALO, ly + HALO, c).to_le_bytes());
                    }
                }
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn run_case(intensity: u8, pdm: PaletteDitherMode) -> String {
    let mut doc = Document::new(DocumentId::new(1), W, H);
    let pid = doc.add_palette("gd4".into(), four_color_palette());
    let params = DitherParamsV2 {
        mode: DitherModeV2::GlitchDither,
        levels: 4,
        glitch_intensity: intensity,
        glitch_seed: 42,
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

#[test]
fn glitch_dither_goldens() {
    let record = std::env::var("GLITCH_DITHER_GOLDEN_RECORD").ok().as_deref() == Some("1");
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for i in [1u8, 5, 12, 20] {
        for (tag, pdm) in [
            ("strict", PaletteDitherMode::Strict),
            ("simple", PaletteDitherMode::Simple),
        ] {
            let key = format!("glitch_dither/i{i}/{tag}");
            let digest = run_case(i, pdm);
            println!("{key} = {digest}");
            got.insert(key, digest);
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
        panic!(
            "EXPECTED is empty — run GLITCH_DITHER_GOLDEN_RECORD=1 and paste tuples into EXPECTED"
        );
    }
    let expected: BTreeMap<_, _> = EXPECTED.iter().copied().collect();
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "case set changed"
    );
    for (k, exp) in &expected {
        let g = got.get(*k).unwrap();
        assert_eq!(g, exp, "glitch_dither golden changed for {k}");
    }
}
