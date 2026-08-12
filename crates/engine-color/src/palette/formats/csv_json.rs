//! CSV and JSON palette format parsers.
//!
//! **CSV format:** One color per line, "r,g,b" (u8 values 0-255).
//! First line may be a header (skipped if non-numeric).
//!
//! **JSON format:** Either an array of objects `[{"r": N, "g": N, "b": N}, ...]`
//! or a flat array of arrays `[[r, g, b], ...]` where N is 0-255.

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

// ─── CSV Parser ───────────────────────────────────────────────────────────────

/// Parse CSV data into sRGB (u8, u8, u8) triples.
pub fn parse_csv(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let text = std::str::from_utf8(data)
        .map_err(|e| parse_error("CSV", "byte 0", &format!("invalid UTF-8: {}", e)))?;

    let mut colors = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip header line: if first non-empty line contains non-numeric first field
        if colors.is_empty() && is_header_line(trimmed) {
            continue;
        }

        match parse_csv_line(trimmed) {
            Ok(color) => colors.push(color),
            Err(reason) => {
                return Err(parse_error("CSV", &format!("line {}", line_num), &reason));
            }
        }
    }

    Ok(colors)
}

/// Check if a line looks like a header (first field contains non-digit characters).
fn is_header_line(line: &str) -> bool {
    let first_field = line.split(',').next().unwrap_or("").trim();
    // A header has alphabetic characters; a pure numeric field (even invalid u8) is not a header
    first_field.chars().any(|c| c.is_alphabetic())
}

/// Parse a single CSV line "r,g,b" into a color triple.
fn parse_csv_line(line: &str) -> Result<(u8, u8, u8), String> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 3 {
        return Err(format!("expected 3 comma-separated values, got {}", parts.len()));
    }

    let r = parts[0]
        .trim()
        .parse::<u8>()
        .map_err(|e| format!("invalid red value '{}': {}", parts[0].trim(), e))?;
    let g = parts[1]
        .trim()
        .parse::<u8>()
        .map_err(|e| format!("invalid green value '{}': {}", parts[1].trim(), e))?;
    let b = parts[2]
        .trim()
        .parse::<u8>()
        .map_err(|e| format!("invalid blue value '{}': {}", parts[2].trim(), e))?;

    Ok((r, g, b))
}

// ─── JSON Parser ──────────────────────────────────────────────────────────────

/// Parse JSON data into sRGB (u8, u8, u8) triples.
///
/// Accepts two formats:
/// - Array of objects: `[{"r": N, "g": N, "b": N}, ...]`
/// - Array of arrays: `[[r, g, b], ...]`
pub fn parse_json(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let text = std::str::from_utf8(data)
        .map_err(|e| parse_error("JSON", "byte 0", &format!("invalid UTF-8: {}", e)))?;

    // Simple manual JSON parser — avoids serde_json dependency for this basic format.
    let trimmed = text.trim();

    if !trimmed.starts_with('[') {
        return Err(parse_error("JSON", "byte 0", "expected JSON array"));
    }

    // Determine format by finding first non-whitespace char after opening bracket
    let inner = trimmed[1..].trim_start();
    if inner.is_empty() || inner.starts_with(']') {
        // Empty array
        return Ok(Vec::new());
    }

    if inner.starts_with('{') {
        parse_json_objects(trimmed)
    } else if inner.starts_with('[') {
        parse_json_arrays(trimmed)
    } else {
        Err(parse_error(
            "JSON",
            "byte 1",
            "expected array of objects or array of arrays",
        ))
    }
}

/// Parse `[{"r": N, "g": N, "b": N}, ...]` format.
fn parse_json_objects(text: &str) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let mut colors = Vec::new();
    let mut pos = 0;

    // Skip opening '['
    pos = skip_to(text, pos, '[') + 1;

    loop {
        pos = skip_whitespace(text, pos);
        if pos >= text.len() {
            break;
        }
        if text.as_bytes()[pos] == b']' {
            break;
        }
        if text.as_bytes()[pos] == b',' {
            pos += 1;
            continue;
        }

        // Parse object
        if text.as_bytes()[pos] != b'{' {
            return Err(parse_error(
                "JSON",
                &format!("char {}", pos),
                "expected '{'",
            ));
        }
        pos += 1;

        let (r, g, b, new_pos) = parse_rgb_object(text, pos)?;
        colors.push((r, g, b));
        pos = new_pos;
    }

    Ok(colors)
}

/// Parse the contents of a `{"r": N, "g": N, "b": N}` object.
/// Returns (r, g, b, position_after_closing_brace).
fn parse_rgb_object(text: &str, start: usize) -> Result<(u8, u8, u8, usize), PaletteError> {
    let mut r: Option<u8> = None;
    let mut g: Option<u8> = None;
    let mut b: Option<u8> = None;
    let mut pos = start;

    loop {
        pos = skip_whitespace(text, pos);
        if pos >= text.len() {
            return Err(parse_error("JSON", &format!("char {}", pos), "unexpected end in object"));
        }
        if text.as_bytes()[pos] == b'}' {
            pos += 1;
            break;
        }
        if text.as_bytes()[pos] == b',' {
            pos += 1;
            continue;
        }

        // Parse key
        let (key, new_pos) = parse_json_string(text, pos)?;
        pos = new_pos;

        // Skip colon
        pos = skip_whitespace(text, pos);
        if pos >= text.len() || text.as_bytes()[pos] != b':' {
            return Err(parse_error("JSON", &format!("char {}", pos), "expected ':'"));
        }
        pos += 1;

        // Parse value (number)
        pos = skip_whitespace(text, pos);
        let (value, new_pos) = parse_json_number(text, pos)?;
        pos = new_pos;

        let val_u8 = value.clamp(0, 255) as u8;
        match key.as_str() {
            "r" | "R" | "red" | "Red" => r = Some(val_u8),
            "g" | "G" | "green" | "Green" => g = Some(val_u8),
            "b" | "B" | "blue" | "Blue" => b = Some(val_u8),
            _ => {} // Skip unknown keys
        }
    }

    let r = r.ok_or_else(|| parse_error("JSON", &format!("char {}", start), "missing 'r' field"))?;
    let g = g.ok_or_else(|| parse_error("JSON", &format!("char {}", start), "missing 'g' field"))?;
    let b = b.ok_or_else(|| parse_error("JSON", &format!("char {}", start), "missing 'b' field"))?;

    Ok((r, g, b, pos))
}

/// Parse `[[r, g, b], ...]` format.
fn parse_json_arrays(text: &str) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let mut colors = Vec::new();
    let mut pos = 0;

    // Skip opening '['
    pos = skip_to(text, pos, '[') + 1;

    loop {
        pos = skip_whitespace(text, pos);
        if pos >= text.len() {
            break;
        }
        if text.as_bytes()[pos] == b']' {
            break;
        }
        if text.as_bytes()[pos] == b',' {
            pos += 1;
            continue;
        }

        // Parse inner array [r, g, b]
        if text.as_bytes()[pos] != b'[' {
            return Err(parse_error(
                "JSON",
                &format!("char {}", pos),
                "expected '['",
            ));
        }
        pos += 1;

        let mut values = Vec::new();
        loop {
            pos = skip_whitespace(text, pos);
            if pos >= text.len() {
                return Err(parse_error("JSON", &format!("char {}", pos), "unexpected end in array"));
            }
            if text.as_bytes()[pos] == b']' {
                pos += 1;
                break;
            }
            if text.as_bytes()[pos] == b',' {
                pos += 1;
                continue;
            }
            let (value, new_pos) = parse_json_number(text, pos)?;
            values.push(value);
            pos = new_pos;
        }

        if values.len() < 3 {
            return Err(parse_error(
                "JSON",
                &format!("char {}", pos),
                &format!("expected 3 values in array, got {}", values.len()),
            ));
        }

        let r = values[0].clamp(0, 255) as u8;
        let g = values[1].clamp(0, 255) as u8;
        let b = values[2].clamp(0, 255) as u8;
        colors.push((r, g, b));
    }

    Ok(colors)
}

// ─── JSON Parsing Helpers ────────────────────────────────────────────────────

fn skip_whitespace(text: &str, mut pos: usize) -> usize {
    let bytes = text.as_bytes();
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t' || bytes[pos] == b'\n' || bytes[pos] == b'\r') {
        pos += 1;
    }
    pos
}

fn skip_to(text: &str, mut pos: usize, ch: char) -> usize {
    let bytes = text.as_bytes();
    while pos < bytes.len() && bytes[pos] != ch as u8 {
        pos += 1;
    }
    pos
}

/// Parse a JSON string (including quotes). Returns (string_content, position_after_closing_quote).
fn parse_json_string(text: &str, start: usize) -> Result<(String, usize), PaletteError> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'"' {
        return Err(parse_error("JSON", &format!("char {}", start), "expected '\"'"));
    }

    let mut pos = start + 1;
    let mut result = String::new();

    while pos < bytes.len() {
        match bytes[pos] {
            b'"' => {
                return Ok((result, pos + 1));
            }
            b'\\' => {
                pos += 1;
                if pos < bytes.len() {
                    result.push(bytes[pos] as char);
                }
            }
            c => {
                result.push(c as char);
            }
        }
        pos += 1;
    }

    Err(parse_error("JSON", &format!("char {}", start), "unterminated string"))
}

/// Parse a JSON number (integer). Returns (value, position_after_number).
fn parse_json_number(text: &str, start: usize) -> Result<(i32, usize), PaletteError> {
    let bytes = text.as_bytes();
    let mut pos = start;
    let mut negative = false;

    if pos < bytes.len() && bytes[pos] == b'-' {
        negative = true;
        pos += 1;
    }

    let num_start = pos;
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    // Skip decimal portion if present (just ignore it for integer parsing)
    if pos < bytes.len() && bytes[pos] == b'.' {
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    if num_start == pos && !negative {
        return Err(parse_error("JSON", &format!("char {}", start), "expected number"));
    }

    let num_str = &text[start..pos];
    // Parse as f64 first to handle decimals, then truncate to i32
    let value: f64 = num_str.parse().map_err(|e| {
        parse_error("JSON", &format!("char {}", start), &format!("invalid number: {}", e))
    })?;

    Ok((value as i32, pos))
}

// ─── CSV Exporter ─────────────────────────────────────────────────────────────

/// Export linear colors to CSV format ("r,g,b\n" header then "R,G,B\n" per color).
pub fn export_csv(colors: &[LinearColor]) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut output = String::new();
    output.push_str("r,g,b\n");
    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        output.push_str(&format!("{},{},{}\n", r, g, b));
    }

    Ok(output.into_bytes())
}

// ─── JSON Exporter ────────────────────────────────────────────────────────────

/// Export linear colors to JSON format (`[{"r": N, "g": N, "b": N}, ...]`).
pub fn export_json(colors: &[LinearColor]) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut output = String::from("[\n");
    for (i, color) in colors.iter().enumerate() {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        if i > 0 {
            output.push_str(",\n");
        }
        output.push_str(&format!("  {{\"r\": {}, \"g\": {}, \"b\": {}}}", r, g, b));
    }
    output.push_str("\n]\n");

    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── CSV Tests ─────────────────────────────────────────────────────

    #[test]
    fn csv_basic() {
        let data = b"255,0,0\n0,255,0\n0,0,255\n";
        let colors = parse_csv(data).unwrap();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0], (255, 0, 0));
        assert_eq!(colors[1], (0, 255, 0));
        assert_eq!(colors[2], (0, 0, 255));
    }

    #[test]
    fn csv_with_header() {
        let data = b"r,g,b\n128,64,32\n";
        let colors = parse_csv(data).unwrap();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0], (128, 64, 32));
    }

    #[test]
    fn csv_with_spaces() {
        let data = b"128, 64, 32\n";
        let colors = parse_csv(data).unwrap();
        assert_eq!(colors[0], (128, 64, 32));
    }

    #[test]
    fn csv_empty_lines_skipped() {
        let data = b"0,0,0\n\n255,255,255\n\n";
        let colors = parse_csv(data).unwrap();
        assert_eq!(colors.len(), 2);
    }

    #[test]
    fn csv_invalid_value_fails() {
        let data = b"256,0,0\n";
        assert!(parse_csv(data).is_err());
    }

    // ─── JSON Tests ────────────────────────────────────────────────────

    #[test]
    fn json_objects() {
        let data = br#"[{"r": 255, "g": 0, "b": 128}, {"r": 0, "g": 255, "b": 0}]"#;
        let colors = parse_json(data).unwrap();
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0], (255, 0, 128));
        assert_eq!(colors[1], (0, 255, 0));
    }

    #[test]
    fn json_arrays() {
        let data = b"[[255, 0, 0], [0, 255, 0], [0, 0, 255]]";
        let colors = parse_json(data).unwrap();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0], (255, 0, 0));
        assert_eq!(colors[1], (0, 255, 0));
        assert_eq!(colors[2], (0, 0, 255));
    }

    #[test]
    fn json_empty_array() {
        let data = b"[]";
        let colors = parse_json(data).unwrap();
        assert_eq!(colors.len(), 0);
    }

    #[test]
    fn json_not_array_fails() {
        let data = br#"{"r": 0, "g": 0, "b": 0}"#;
        assert!(parse_json(data).is_err());
    }

    #[test]
    fn json_with_whitespace() {
        let data = br#"
        [
            { "r": 128, "g": 64, "b": 32 }
        ]
        "#;
        let colors = parse_json(data).unwrap();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0], (128, 64, 32));
    }
}
