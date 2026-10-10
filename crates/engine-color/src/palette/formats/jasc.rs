//! JASC-PAL (Paint Shop Pro) `.pal` text palette parser and exporter.
//!
//! Format:
//! ```text
//! JASC-PAL
//! 0100
//! <count>
//! R G B
//! ...
//! ```

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

/// Strip a leading UTF-8 BOM (`EF BB BF` / U+FEFF) if present.
fn strip_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data)
}

/// True when `data` looks like a JASC-PAL text file (not Microsoft RIFF).
pub fn looks_like_jasc(data: &[u8]) -> bool {
    let data = strip_bom(data);
    let head = std::str::from_utf8(data.get(..64).unwrap_or(data))
        .unwrap_or("")
        .trim_start()
        .trim_start_matches('\u{feff}');
    head.to_ascii_uppercase().starts_with("JASC-PAL")
}

/// Parse JASC-PAL text into sRGB color triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let data = strip_bom(data);
    let text = std::str::from_utf8(data)
        .map_err(|e| parse_error("JASC", "byte 0", &format!("invalid UTF-8: {e}")))?;
    let text = text.trim_start_matches('\u{feff}');

    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());

    let magic = lines
        .next()
        .ok_or_else(|| parse_error("JASC", "line 1", "empty file"))?;
    if !magic.eq_ignore_ascii_case("JASC-PAL") {
        return Err(parse_error(
            "JASC",
            "line 1",
            &format!("expected 'JASC-PAL', got '{magic}'"),
        ));
    }

    let version = lines
        .next()
        .ok_or_else(|| parse_error("JASC", "line 2", "missing version"))?;
    // Accept "0100" (canonical) and bare "100" seen in some exporters.
    if version != "0100" && version != "100" {
        return Err(parse_error(
            "JASC",
            "line 2",
            &format!("unsupported version '{version}' (expected 0100)"),
        ));
    }

    let count_line = lines
        .next()
        .ok_or_else(|| parse_error("JASC", "line 3", "missing color count"))?;
    let count: usize = count_line.parse().map_err(|e| {
        parse_error(
            "JASC",
            "line 3",
            &format!("invalid color count '{count_line}': {e}"),
        )
    })?;

    let mut colors = Vec::with_capacity(count);
    let mut line_num = 3;
    for line in lines {
        line_num += 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(parse_error(
                "JASC",
                &format!("line {line_num}"),
                &format!("expected R G B, got '{line}'"),
            ));
        }
        let r: u8 = parts[0].parse().map_err(|e| {
            parse_error(
                "JASC",
                &format!("line {line_num}"),
                &format!("invalid red '{}': {e}", parts[0]),
            )
        })?;
        let g: u8 = parts[1].parse().map_err(|e| {
            parse_error(
                "JASC",
                &format!("line {line_num}"),
                &format!("invalid green '{}': {e}", parts[1]),
            )
        })?;
        let b: u8 = parts[2].parse().map_err(|e| {
            parse_error(
                "JASC",
                &format!("line {line_num}"),
                &format!("invalid blue '{}': {e}", parts[2]),
            )
        })?;
        colors.push((r, g, b));
    }

    if colors.len() != count {
        return Err(parse_error(
            "JASC",
            "end of file",
            &format!("header count {count} but found {} colors", colors.len()),
        ));
    }

    Ok(colors)
}

/// Export linear colors as JASC-PAL text.
pub fn export(colors: &[LinearColor], _name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut out = String::with_capacity(32 + colors.len() * 12);
    out.push_str("JASC-PAL\n");
    out.push_str("0100\n");
    out.push_str(&format!("{}\n", colors.len()));
    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        out.push_str(&format!("{r} {g} {b}\n"));
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                g: 0.0,
                b: 1.0,
            },
        ];
        let exported = export(&colors, None).unwrap();
        assert!(looks_like_jasc(&exported));
        let parsed = parse(&exported).unwrap();
        assert_eq!(parsed, vec![(255, 0, 0), (0, 0, 255)]);
    }

    #[test]
    fn count_mismatch_errors() {
        let data = b"JASC-PAL\n0100\n2\n255 0 0\n";
        assert!(parse(data).is_err());
    }

    #[test]
    fn parse_utf8_bom_and_crlf() {
        let data = b"\xEF\xBB\xBFJASC-PAL\r\n0100\r\n2\r\n255 0 0\r\n0 0 255\r\n";
        assert!(looks_like_jasc(data));
        let parsed = parse(data).unwrap();
        assert_eq!(parsed, vec![(255, 0, 0), (0, 0, 255)]);
    }

    #[test]
    fn parse_case_insensitive_magic() {
        let data = b"jasc-pal\n0100\n1\n10 20 30\n";
        assert!(looks_like_jasc(data));
        assert_eq!(parse(data).unwrap(), vec![(10, 20, 30)]);
    }
}
