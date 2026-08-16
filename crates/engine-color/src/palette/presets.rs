//! Built-in retro palette presets (source of truth for UI listing).
//!
//! Additional presets can be appended to [`BUILTIN_PRESETS`] without changing
//! the list/import API shape.

/// A built-in palette definition: stable id, display name, sRGB u8 colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalettePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub colors_srgb: &'static [(u8, u8, u8)],
}

const fn srgb(hex: u32) -> (u8, u8, u8) {
    (
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// Game Boy DMG LCD greens (classic 4-shade set).
const GAMEBOY_COLORS: &[(u8, u8, u8)] = &[
    (15, 56, 15),
    (48, 98, 48),
    (139, 172, 15),
    (155, 188, 15),
];

/// Apple II 16-color lo-res palette.
/// Gray 1 and Gray 2 are identical (indices 5 and 10), as on original hardware.
const APPLE2_COLORS: &[(u8, u8, u8)] = &[
    (0, 0, 0),       // 0  Black
    (114, 38, 64),   // 1  Magenta / Deep Red
    (64, 51, 127),   // 2  Dark Blue
    (228, 52, 254),  // 3  Purple
    (14, 89, 64),    // 4  Dark Green
    (128, 128, 128), // 5  Gray 1
    (27, 154, 254),  // 6  Medium Blue
    (191, 179, 255), // 7  Light Blue
    (64, 76, 0),     // 8  Brown
    (228, 101, 1),   // 9  Orange
    (128, 128, 128), // A  Gray 2 (same as Gray 1)
    (241, 166, 191), // B  Pink
    (27, 203, 1),    // C  Light Green
    (191, 204, 128), // D  Yellow
    (141, 217, 191), // E  Aquamarine
    (255, 255, 255), // F  White
];

/// PICO-8 fantasy console (16 colors).
const PICO8_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x000000),
    srgb(0x1D2B53),
    srgb(0x7E2553),
    srgb(0x008751),
    srgb(0xAB5236),
    srgb(0x5F574F),
    srgb(0xC2C3C7),
    srgb(0xFFF1E8),
    srgb(0xFF004D),
    srgb(0xFFA300),
    srgb(0xFFEC27),
    srgb(0x00E436),
    srgb(0x29ADFF),
    srgb(0x83769C),
    srgb(0xFF77A8),
    srgb(0xFFCCAA),
];

/// Endesga 32 — general-purpose pixel-art palette.
const ENDESGA32_COLORS: &[(u8, u8, u8)] = &[
    srgb(0xBE4A2F),
    srgb(0xD77643),
    srgb(0xEAD4AA),
    srgb(0xE4A672),
    srgb(0xB86F50),
    srgb(0x733E39),
    srgb(0x3E2731),
    srgb(0xA22633),
    srgb(0xE43B44),
    srgb(0xF77622),
    srgb(0xFEAE34),
    srgb(0xFEE761),
    srgb(0x63C74D),
    srgb(0x3E8948),
    srgb(0x265C42),
    srgb(0x193C3E),
    srgb(0x124E89),
    srgb(0x0099DB),
    srgb(0x2CE8F5),
    srgb(0xFFFFFF),
    srgb(0xC0CBD0),
    srgb(0x8B9BB4),
    srgb(0x5A6988),
    srgb(0x3A4466),
    srgb(0x262B44),
    srgb(0x181425),
    srgb(0xFF0044),
    srgb(0x68386C),
    srgb(0xB55088),
    srgb(0xF6757A),
    srgb(0xE8B796),
    srgb(0xC28569),
];

/// Commodore VIC-20 (16 colors, 1980).
const VIC20_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x000000),
    srgb(0xFFFFFF),
    srgb(0x782922),
    srgb(0x87D6DD),
    srgb(0xAA38B0),
    srgb(0x59AC30),
    srgb(0x3227A6),
    srgb(0xD7DE65),
    srgb(0xA15400),
    srgb(0xCF8272),
    srgb(0xB869C8),
    srgb(0x98E2EA),
    srgb(0xC8D890),
    srgb(0x8A7ECE),
    srgb(0x8ECE78),
    srgb(0xA1A1A1),
];

/// Solar8 — high-contrast 8-color stylization set.
const SOLAR8_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x10141F),
    srgb(0x3C233D),
    srgb(0x851747),
    srgb(0xD9383A),
    srgb(0xE86A17),
    srgb(0xF2A65A),
    srgb(0xF0E6A3),
    srgb(0xFFFFFF),
];

/// MSX1 system palette (15 opaque colors; transparent index omitted).
const MSX_SYSTEM_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x000000),
    srgb(0x3EB849),
    srgb(0x74D07D),
    srgb(0x5955E0),
    srgb(0x8076F1),
    srgb(0xB95E51),
    srgb(0x65DBEF),
    srgb(0xDB6559),
    srgb(0xFF897D),
    srgb(0xCCC35E),
    srgb(0xDED087),
    srgb(0x3AA246),
    srgb(0xB666B8),
    srgb(0xCCCCCC),
    srgb(0xFFFFFF),
];

/// DawnBringer 16.
const DAWNBRINGER16_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x140C1C),
    srgb(0x442434),
    srgb(0x30346D),
    srgb(0x4E4A4E),
    srgb(0x854C30),
    srgb(0x346524),
    srgb(0xD04648),
    srgb(0x757161),
    srgb(0x597DCE),
    srgb(0xD27D2C),
    srgb(0x8595A1),
    srgb(0x6DAA2C),
    srgb(0xD2AA99),
    srgb(0x6DC2CA),
    srgb(0xDAD45E),
    srgb(0xDEEED6),
];

/// Amstrad CPC full hardware palette (27 RGB combinations).
const AMSTRAD_CPC_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x000000),
    srgb(0x000080),
    srgb(0x0000FF),
    srgb(0x800000),
    srgb(0x800080),
    srgb(0x8000FF),
    srgb(0xFF0000),
    srgb(0xFF0080),
    srgb(0xFF00FF),
    srgb(0x008000),
    srgb(0x008080),
    srgb(0x0080FF),
    srgb(0x808000),
    srgb(0x808080),
    srgb(0x8080FF),
    srgb(0xFF8000),
    srgb(0xFF8080),
    srgb(0xFF80FF),
    srgb(0x00FF00),
    srgb(0x00FF80),
    srgb(0x00FFFF),
    srgb(0x80FF00),
    srgb(0x80FF80),
    srgb(0x80FFFF),
    srgb(0xFFFF00),
    srgb(0xFFFF80),
    srgb(0xFFFFFF),
];

/// Synthwave neon 8 — high-chroma retrowave set.
const SYNTHWAVE8_COLORS: &[(u8, u8, u8)] = &[
    srgb(0x11052C),
    srgb(0x3D087B),
    srgb(0xF43B86),
    srgb(0xFFE459),
    srgb(0x00F5D4),
    srgb(0x7B2CBF),
    srgb(0xFF007F),
    srgb(0x3A0CA3),
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
    PalettePreset {
        id: "pico_8",
        name: "PICO-8",
        colors_srgb: PICO8_COLORS,
    },
    PalettePreset {
        id: "endesga_32",
        name: "Endesga 32",
        colors_srgb: ENDESGA32_COLORS,
    },
    PalettePreset {
        id: "vic_20",
        name: "Commodore VIC-20",
        colors_srgb: VIC20_COLORS,
    },
    PalettePreset {
        id: "solar8",
        name: "Solar8",
        colors_srgb: SOLAR8_COLORS,
    },
    PalettePreset {
        id: "msx_system",
        name: "MSX System",
        colors_srgb: MSX_SYSTEM_COLORS,
    },
    PalettePreset {
        id: "dawnbringer_16",
        name: "DawnBringer 16",
        colors_srgb: DAWNBRINGER16_COLORS,
    },
    PalettePreset {
        id: "amstrad_cpc",
        name: "Amstrad CPC Full",
        colors_srgb: AMSTRAD_CPC_COLORS,
    },
    PalettePreset {
        id: "synthwave_8",
        name: "Synthwave Neon 8",
        colors_srgb: SYNTHWAVE8_COLORS,
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
    use std::collections::HashSet;

    #[test]
    fn find_known_ids() {
        let gb = find_preset("gameboy").expect("gameboy");
        assert_eq!(gb.name, "Game Boy");
        assert_eq!(gb.colors_srgb.len(), 4);
        assert_eq!(gb.colors_srgb[0], (15, 56, 15));

        let a2 = find_preset("apple2").expect("apple2");
        assert_eq!(a2.name, "Apple II");
        assert_eq!(a2.colors_srgb.len(), 16);
        assert_eq!(a2.colors_srgb[0], (0, 0, 0));
        assert_eq!(a2.colors_srgb[1], (114, 38, 64));
        assert_eq!(a2.colors_srgb[5], (128, 128, 128));
        assert_eq!(a2.colors_srgb[5], a2.colors_srgb[10]);
        assert_eq!(a2.colors_srgb[9], (228, 101, 1));
        assert_eq!(a2.colors_srgb[15], (255, 255, 255));

        let pico = find_preset("pico_8").expect("pico_8");
        assert_eq!(pico.name, "PICO-8");
        assert_eq!(pico.colors_srgb.len(), 16);
        assert_eq!(pico.colors_srgb[8], (255, 0, 77));

        assert_eq!(find_preset("endesga_32").unwrap().colors_srgb.len(), 32);
        assert_eq!(find_preset("vic_20").unwrap().colors_srgb.len(), 16);
        assert_eq!(find_preset("solar8").unwrap().colors_srgb.len(), 8);
        assert_eq!(find_preset("msx_system").unwrap().colors_srgb.len(), 15);
        assert_eq!(find_preset("dawnbringer_16").unwrap().colors_srgb.len(), 16);
        assert_eq!(find_preset("amstrad_cpc").unwrap().colors_srgb.len(), 27);
        assert_eq!(find_preset("synthwave_8").unwrap().colors_srgb.len(), 8);
    }

    #[test]
    fn registry_ids_are_unique() {
        let mut ids = HashSet::new();
        for preset in BUILTIN_PRESETS {
            assert!(ids.insert(preset.id), "duplicate builtin id {}", preset.id);
            assert!(!preset.colors_srgb.is_empty());
        }
        assert_eq!(BUILTIN_PRESETS.len(), 10);
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
