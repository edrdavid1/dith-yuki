//! Baseline golden hashes for existing ED modes (Strict + Simple).
//!
//! Captured **before** Ed1d / `modulated_diffuse_y` lands. If any expected
//! hash changes after that work without an intentional FS edit, stop and report.
//!
//! Record mode: `ED_BASELINE_RECORD=1 cargo test -p engine-project --test ed_baseline_goldens -- --nocapture`

use std::collections::BTreeMap;

use blake3::Hasher;
use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};
use engine_project::filters::dither_diffusion::apply_error_diffusion;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

/// Expected blake3 hex digests (core 256² RGB only). Captured before Ed1d.
const EXPECTED: &[(&str, &str)] = &[
    (
        "atkinson/simple",
        "d1071961bd575f340c88e4dde648710918f18ecfe577c6be24e6d3b51d0490c0",
    ),
    (
        "atkinson/strict",
        "2096e2450e4aaeae3bf0e0f01beb8358a8171c2467906155c36486ff3384cb33",
    ),
    (
        "burkes/simple",
        "63d0f3d286bffc17e8b42a79109c0b0428406901bbf704266ec4382260339c95",
    ),
    (
        "burkes/strict",
        "148292797705bccf8923182134d6d929ec4e256cda439198266998b01ec3f0a0",
    ),
    (
        "floyd_steinberg/simple",
        "0b4b8b1d6da9bdceaee35cb56149e5ac65f6180fce78edf88fb7086b66584136",
    ),
    (
        "floyd_steinberg/strict",
        "1a134053c4d1ebc15b729b9c695a250eba9b942439738b60432f2e03bd408a7e",
    ),
    (
        "jarvis_judice_ninke/simple",
        "ec223d41355ca65c8b0b2c76853e6708f177a274197449cd3d290c2d8c11cebd",
    ),
    (
        "jarvis_judice_ninke/strict",
        "d5f4a7a93024f2479b55b8b09ed64b1809b551e61fb2b3662b0691500a76ccea",
    ),
    (
        "sierra/simple",
        "03c92343098a6c74d0a9396d551a9766bfcabdb5559ad6596764d2ac022387c9",
    ),
    (
        "sierra/strict",
        "2e72daf071138a1ab42b7ba1d51039330e991e0813bc9679c1f55f36642f3d1f",
    ),
    (
        "stucki/simple",
        "785d5b9aa14ca1687042023c832caa5dd5802b7ca55b3ef939cf2321391eaf4e",
    ),
    (
        "stucki/strict",
        "a0f635eabf017e78d9c9399a63d4832e49b5646b2a48f2c5a9d7870e63b2985e",
    ),
];

fn fill_gradient_tile(coord: TileCoord) -> PixelTile {
    let mut t = PixelTile::new();
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let gx = (coord.x * TILE_SIZE + x.saturating_sub(HALO)) as f32;
            let gy = (coord.y * TILE_SIZE + y.saturating_sub(HALO)) as f32;
            let u = (gx / 512.0).clamp(0.0, 1.0);
            let v = (gy / 512.0).clamp(0.0, 1.0);
            // Mild "photo-like" modulation so Simple/Strict diverge meaningfully.
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

fn hash_tile_core(tile: &PixelTile) -> String {
    let mut h = Hasher::new();
    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            for c in 0..3u32 {
                h.update(&tile.at(x, y, c).to_le_bytes());
            }
        }
    }
    h.finalize().to_hex().to_string()
}

fn run_mode(mode: DitherModeV2, pdm: PaletteDitherMode) -> String {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let pid = doc.add_palette("baseline4".into(), four_color_palette());
    let params = DitherParamsV2 {
        mode,
        levels: 4,
        palette_id: Some(pid),
        palette_dither_mode: pdm,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let src = fill_gradient_tile(coord);
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();
    let rs = ErrorResidualsStore::new();
    let out = apply_error_diffusion(&src, coord, &params, &rs, LayerId::new(1), &pc, &lc, &doc)
        .expect("ed apply");
    hash_tile_core(&out)
}

fn cases() -> Vec<(String, DitherModeV2, PaletteDitherMode)> {
    let modes = [
        DitherModeV2::FloydSteinberg,
        DitherModeV2::Atkinson,
        DitherModeV2::Stucki,
        DitherModeV2::JarvisJudiceNinke,
        DitherModeV2::Burkes,
        DitherModeV2::Sierra,
    ];
    let mut out = Vec::new();
    for m in modes {
        let name = m.algorithm_id().unwrap_or("?");
        out.push((
            format!("{name}/strict"),
            m.clone(),
            PaletteDitherMode::Strict,
        ));
        out.push((format!("{name}/simple"), m, PaletteDitherMode::Simple));
    }
    out
}

#[test]
fn ed_baseline_goldens_unchanged() {
    let record = std::env::var("ED_BASELINE_RECORD").ok().as_deref() == Some("1");
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for (key, mode, pdm) in cases() {
        let digest = run_mode(mode, pdm);
        println!("{key} = {digest}");
        got.insert(key, digest);
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
            "EXPECTED is empty — run once with ED_BASELINE_RECORD=1 before Ed1d changes, \
             then paste the printed tuples into EXPECTED"
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
        assert_eq!(
            g, exp,
            "baseline golden changed for {k}: got {g}, expected {exp}"
        );
    }
}
