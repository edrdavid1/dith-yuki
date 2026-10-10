//! Lospec `.hex` palette parser and exporter.
//!
//! Text format used by [Lospec](https://lospec.com/palette-list): one hex color
//! per line (`RRGGBB` or `#RRGGBB`). Blank lines and `# …` comment lines are
//! skipped.

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

/// Parse Lospec `.hex` bytes into sRGB color triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let text = std::str::from_utf8(data)
        .map_err(|e| parse_error("HEX", "byte 0", &format!("invalid UTF-8: {e}")))?;

    let mut colors = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // `#RRGGBB` is a color; `# comment` is skipped.
        let hex = if let Some(rest) = trimmed.strip_prefix('#') {
            if rest.len() == 6 && rest.chars().all(|c| c.is_ascii_hexdigit()) {
                rest
            } else {
                continue;
            }
        } else {
            trimmed
        };

        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(parse_error(
                "HEX",
                &format!("line {}", i + 1),
                &format!("expected RRGGBB hex color, got '{trimmed}'"),
            ));
        }

        let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| {
            parse_error("HEX", &format!("line {}", i + 1), &format!("invalid red: {e}"))
        })?;
        let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| {
            parse_error(
                "HEX",
                &format!("line {}", i + 1),
                &format!("invalid green: {e}"),
            )
        })?;
        let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| {
            parse_error(
                "HEX",
                &format!("line {}", i + 1),
                &format!("invalid blue: {e}"),
            )
        })?;
        colors.push((r, g, b));
    }

    Ok(colors)
}

/// Export linear colors as Lospec `.hex` (one `RRGGBB` per line, lowercase).
pub fn export(colors: &[LinearColor], _name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut out = String::with_capacity(colors.len() * 7);
    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        out.push_str(&format!("{r:02x}{g:02x}{b:02x}\n"));
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lospec_sample() {
        let data = b"1a1c2c\n5d275d\nb13e53\nef7d57\nffcd75\na7f070\n38b764\n257179\n29366f\n3b5dc9\n41a6f6\n73eff7\nf4f4f4\n94b0c2\n566c86\n333c57\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors.len(), 16);
        assert_eq!(colors[0], (0x1a, 0x1c, 0x2c));
        assert_eq!(colors[12], (0xf4, 0xf4, 0xf4));
    }

    #[test]
    fn parse_with_hash_prefix_and_comments() {
        let data = b"# Pico-8 style\n#FF004D\n29ADFF\n\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors, vec![(0xff, 0x00, 0x4d), (0x29, 0xad, 0xff)]);
    }

    #[test]
    fn round_trip() {
        let colors = vec![
            LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            },
        ];
        let exported = export(&colors, None).unwrap();
        let parsed = parse(&exported).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], (255, 0, 0));
        assert_eq!(parsed[1], (0, 255, 0));
    }

    #[test]
    fn export_empty_errors() {
        assert!(export(&[], None).is_err());
    }
}
