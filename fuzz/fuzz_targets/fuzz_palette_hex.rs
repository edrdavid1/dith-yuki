//! Fuzz: Lospec `.hex` parse + build linear palette colors.

#![no_main]
use engine_color::palette::formats::parse_format;
use engine_color::palette::{srgb_to_linear, LinearColor, PaletteFormat};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    let Ok(colors) = parse_format(data, PaletteFormat::Hex) else {
        return;
    };
    // Build palette colors (same path as import_palette after parse).
    let _palette: Vec<LinearColor> = colors
        .into_iter()
        .map(|(r, g, b)| LinearColor {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
        })
        .collect();
});
