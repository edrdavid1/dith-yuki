//! Fuzz: `.pal` parse (JASC + RIFF autodetect) + build linear palette colors.

#![no_main]
use engine_color::palette::formats::parse_format;
use engine_color::palette::{srgb_to_linear, LinearColor, PaletteFormat};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    // PaletteFormat::Pal autodetects JASC text vs RIFF PAL (and GPL fallback).
    let Ok(colors) = parse_format(data, PaletteFormat::Pal) else {
        return;
    };
    let _palette: Vec<LinearColor> = colors
        .into_iter()
        .map(|(r, g, b)| LinearColor {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
        })
        .collect();
    // Explicit JASC path as well (same bytes may be valid for both).
    let _ = parse_format(data, PaletteFormat::Jasc);
});
