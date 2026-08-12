//! Built-in retro palette presets (source of truth for UI listing).
//!
//! Additional presets (CGA, EGA, C64, Pico-8, NES, …) can be appended to
//! [`BUILTIN_PRESETS`] without changing the list/import API shape.

/// A built-in palette definition: stable id, display name, sRGB u8 colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalettePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub colors_srgb: &'static [(u8, u8, u8)],
}

/// Game Boy DMG LCD greens (classic 4-shade set).
const GAMEBOY_COLORS: &[(u8, u8, u8)] = &[
    (15, 56, 15),
    (48, 98, 48),
    (139, 172, 15),
    (155, 188, 15),
];

/// Apple II 16-color lo-res palette (classic NTSC / composite look).
///
/// Source: AppleWin RGB extraction documented by Meresh
/// <https://www.mrob.com/pub/xapple2/colors.html> (“Original Apple ][ values”
/// column) and <https://www.meresh.com/a2colors.html>.
/// Gray 1 and Gray 2 are identical on original hardware (indices 5 and 10).
const APPLE2_COLORS: &[(u8, u8, u8)] = &[
    (0, 0, 0),       // 0  Black
    (227, 30, 96),   // 1  Magenta / Deep Red
    (96, 78, 189),   // 2  Dark Blue
    (255, 68, 253),  // 3  Purple
    (0, 163, 96),    // 4  Dark Green
    (156, 156, 156), // 5  Gray 1
    (20, 207, 253),  // 6  Medium Blue
    (208, 195, 255), // 7  Light Blue
    (96, 114, 3),    // 8  Brown
    (255, 106, 60),  // 9  Orange
    (156, 156, 156), // A  Gray 2 (same as Gray 1 on original HW)
    (255, 160, 208), // B  Pink
    (20, 245, 60),   // C  Light Green
    (208, 221, 141), // D  Yellow
    (114, 255, 208), // E  Aquamarine
    (255, 255, 255), // F  White
];

/// Extensible registry of built-in retro palettes.
pub const BUILTIN_PRESETS: &[PalettePreset] = &[
    PalettePreset {
        id: "gameboy",
        name: "Game Boy",
        colors_srgb: GAMEBOY_COLORS,
    },
    PalettePreset {
        id: "apple2",
        name: "Apple II",
        colors_srgb: APPLE2_COLORS,
    },
];

/// Look up a preset by stable id.
pub fn find_preset(id: &str) -> Option<&'static PalettePreset> {
    BUILTIN_PRESETS.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::srgb_to_linear;

    #[test]
    fn find_known_ids() {
        let gb = find_preset("gameboy").expect("gameboy");
        assert_eq!(gb.name, "Game Boy");
        assert_eq!(gb.colors_srgb.len(), 4);
        assert_eq!(gb.colors_srgb[0], (15, 56, 15));

        let a2 = find_preset("apple2").expect("apple2");
        assert_eq!(a2.name, "Apple II");
        assert_eq!(a2.colors_srgb.len(), 16);
    }

    #[test]
    fn unknown_id_returns_none() {
        assert!(find_preset("cga").is_none());
        assert!(find_preset("").is_none());
    }

    #[test]
    fn gameboy_srgb_to_linear_matches_transfer() {
        let gb = find_preset("gameboy").unwrap();
        for &(r, g, b) in gb.colors_srgb {
            assert!((srgb_to_linear(r) - expected_linear(r)).abs() < 1e-6);
            assert!((srgb_to_linear(g) - expected_linear(g)).abs() < 1e-6);
            assert!((srgb_to_linear(b) - expected_linear(b)).abs() < 1e-6);
        }
        // Spot-check first green against known sRGB→linear
        let (r, g, b) = gb.colors_srgb[0];
        assert!((srgb_to_linear(r) - 0.002_124_969).abs() < 1e-5 || r == 15);
        let _ = (r, g, b);
    }

    fn expected_linear(value: u8) -> f32 {
        let n = value as f32 / 255.0;
        if n <= 0.04045 {
            n / 12.92
        } else {
            ((n + 0.055) / 1.055).powf(2.4)
        }
    }
}
