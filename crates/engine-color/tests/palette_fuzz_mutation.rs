//! CI substitute for `cargo fuzz` on Lospec `.hex` and JASC/RIFF `.pal` parsers.
//!
//! Seeds live under `tests/fixtures/palettes/`. Local libFuzzer:
//! ```text
//! cp crates/engine-color/tests/fixtures/palettes/*.hex fuzz/corpus/fuzz_palette_hex/
//! cp crates/engine-color/tests/fixtures/palettes/*.pal fuzz/corpus/fuzz_palette_pal/
//! cd fuzz && cargo fuzz run fuzz_palette_hex -- -max_total_time=30
//! cd fuzz && cargo fuzz run fuzz_palette_pal -- -max_total_time=30
//! ```

use engine_color::palette::formats::parse_format;
use engine_color::palette::{srgb_to_linear, LinearColor, PaletteError, PaletteFormat};
use std::path::PathBuf;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/palettes")
}

fn load(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_root().join(name)).unwrap_or_else(|e| panic!("load {name}: {e}"))
}

fn mutations(seed: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if seed.is_empty() {
        out.push(vec![]);
        out.push(vec![0]);
        return out;
    }
    for keep in [0usize, 1, 4, seed.len() / 2, seed.len().saturating_sub(1)] {
        if keep < seed.len() {
            out.push(seed[..keep].to_vec());
        }
    }
    for &idx in &[0usize, 1, seed.len() / 3, seed.len().saturating_sub(1)] {
        if idx < seed.len() {
            let mut m = seed.to_vec();
            m[idx] ^= 0xff;
            out.push(m);
        }
    }
    if seed.len() > 4 {
        let mut m = seed.to_vec();
        m.splice(2..2, [0u8, 0xff, b'\n']);
        out.push(m);
    }
    out.push(vec![]);
    out.push(b"RIFF".to_vec());
    out.push(b"JASC-PAL\n".to_vec());
    out.push(b"gg0000\n".to_vec());
    out
}

fn parse_and_build(data: &[u8], format: PaletteFormat) -> Result<Vec<LinearColor>, PaletteError> {
    let colors = parse_format(data, format)?;
    Ok(colors
        .into_iter()
        .map(|(r, g, b)| LinearColor {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
        })
        .collect())
}

#[test]
fn seed_hex_parses_and_builds() {
    let colors = parse_and_build(&load("lospec_db16.hex"), PaletteFormat::Hex).unwrap();
    assert_eq!(colors.len(), 16);
}

#[test]
fn seed_jasc_parses_and_builds() {
    let colors = parse_and_build(&load("jasc_rgb.pal"), PaletteFormat::Pal).unwrap();
    assert_eq!(colors.len(), 3);
    let jasc = parse_and_build(&load("jasc_rgb.pal"), PaletteFormat::Jasc).unwrap();
    assert_eq!(jasc.len(), 3);
}

#[test]
fn seed_riff_parses_and_builds() {
    let colors = parse_and_build(&load("riff_one.pal"), PaletteFormat::Pal).unwrap();
    assert_eq!(colors.len(), 1);
}

#[test]
fn malformed_hex_is_typed_error_not_panic() {
    let err = parse_and_build(b"not-a-color\n", PaletteFormat::Hex).unwrap_err();
    assert!(matches!(err, PaletteError::ParseError { .. }));
}

#[test]
fn malformed_pal_is_typed_error_not_panic() {
    let err = parse_and_build(b"NOT-A-PAL\n0100\n1\n0 0 0\n", PaletteFormat::Pal).unwrap_err();
    assert!(matches!(err, PaletteError::ParseError { .. }));
}

#[test]
fn mutated_hex_never_panics() {
    let seed = load("lospec_db16.hex");
    for (i, bytes) in mutations(&seed).into_iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = parse_and_build(&bytes, PaletteFormat::Hex);
        }));
        assert!(result.is_ok(), "hex mutation {i} panicked (len={})", bytes.len());
    }
}

#[test]
fn mutated_pal_never_panics() {
    for name in ["jasc_rgb.pal", "riff_one.pal"] {
        let seed = load(name);
        for (i, bytes) in mutations(&seed).into_iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = parse_and_build(&bytes, PaletteFormat::Pal);
                let _ = parse_and_build(&bytes, PaletteFormat::Jasc);
            }));
            assert!(
                result.is_ok(),
                "{name} mutation {i} panicked (len={})",
                bytes.len()
            );
        }
    }
}
