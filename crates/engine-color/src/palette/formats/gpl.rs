//! GIMP Palette (.gpl) format parser.
//!
//! Text format:
//! - First line: "GIMP Palette"
//! - Optional "Name: ..." and "Columns: ..." header lines
//! - Comment lines start with '#'
//! - Color lines: "R G B" with optional name after (whitespace-separated)
//!   where R, G, B are decimal integers 0–255

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

/// Parse a GPL file from raw bytes into sRGB (u8, u8, u8) triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let text = std::str::from_utf8(data).map_err(|e| {
        parse_error("GPL", "byte 0", &format!("invalid UTF-8: {}", e))
    })?;

    let mut lines = text.lines();

    // First line must be "GIMP Palette"
    let first_line = lines.next().ok_or_else(|| {
        parse_error("GPL", "line 1", "empty file")
    })?;

    if first_line.trim() != "GIMP Palette" {
        return Err(parse_error(
            "GPL",
            "line 1",
            &format!("expected 'GIMP Palette', got '{}'", first_line.trim()),
        ));
    }

    let mut colors = Vec::new();
    let mut line_num = 1;

    for line in lines {
        line_num += 1;
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip comment lines
        if trimmed.starts_with('#') {
            continue;
        }

        // Skip "Name:" and "Columns:" header lines
        if trimmed.starts_with("Name:") || trimmed.starts_with("Columns:") {
            continue;
        }

        // Parse color line: "R G B [name]"
        match parse_color_line(trimmed) {
            Ok(color) => colors.push(color),
            Err(reason) => {
                return Err(parse_error(
                    "GPL",
                    &format!("line {}", line_num),
                    &reason,
                ));
            }
        }
    }

    Ok(colors)
}

/// Parse a single "R G B [name]" line.
fn parse_color_line(line: &str) -> Result<(u8, u8, u8), String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(format!(
            "expected at least 3 values (R G B), got {} tokens",
            parts.len()
        ));
    }

    let r = parts[0]
        .parse::<u8>()
        .map_err(|e| format!("invalid red value '{}': {}", parts[0], e))?;
    let g = parts[1]
        .parse::<u8>()
        .map_err(|e| format!("invalid green value '{}': {}", parts[1], e))?;
    let b = parts[2]
        .parse::<u8>()
        .map_err(|e| format!("invalid blue value '{}': {}", parts[2], e))?;

    Ok((r, g, b))
}

/// Export linear colors to GIMP Palette format bytes.
pub fn export(colors: &[LinearColor], name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut output = String::new();
    output.push_str("GIMP Palette\n");

    if let Some(name) = name {
        // Truncate name to 256 chars
        let truncated: String = name.chars().take(256).collect();
        output.push_str(&format!("Name: {}\n", truncated));
    }

    output.push_str("Columns: 16\n");
    output.push_str("#\n");

    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        output.push_str(&format!("{:>3} {:>3} {:>3}\n", r, g, b));
    }

    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_gpl() {
        let data = b"GIMP Palette\nName: Test\nColumns: 16\n#\n255 0 0\tRed\n0 255 0\tGreen\n0 0 255\tBlue\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0], (255, 0, 0));
        assert_eq!(colors[1], (0, 255, 0));
        assert_eq!(colors[2], (0, 0, 255));
    }

    #[test]
    fn parse_no_header_name() {
        let data = b"GIMP Palette\n128 64 32\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0], (128, 64, 32));
    }

    #[test]
    fn parse_with_comments() {
        let data = b"GIMP Palette\n# This is a comment\n0 0 0\n# Another comment\n255 255 255\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0], (0, 0, 0));
        assert_eq!(colors[1], (255, 255, 255));
    }

    #[test]
    fn parse_missing_header_fails() {
        let data = b"Not a GIMP Palette\n255 0 0\n";
        assert!(parse(data).is_err());
    }

    #[test]
    fn parse_empty_file_fails() {
        let data: &[u8] = b"";
        assert!(parse(data).is_err());
    }

    #[test]
    fn parse_invalid_value_fails() {
        let data = b"GIMP Palette\n256 0 0\n";
        assert!(parse(data).is_err());
    }

    #[test]
    fn parse_tabs_and_spaces() {
        let data = b"GIMP Palette\n  128   64   32  Brownish\n";
        let colors = parse(data).unwrap();
        assert_eq!(colors[0], (128, 64, 32));
    }
}
