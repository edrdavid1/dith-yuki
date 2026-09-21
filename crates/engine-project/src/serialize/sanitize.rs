//! Display-string sanitization for values loaded from archives (SPEC §7.5).
//!
//! Does **not** replace output escaping in UI (§9.8) — both layers are required.

use unicode_normalization::UnicodeNormalization;

/// Sanitize a user-facing string once on archive ingest.
///
/// - NFC normalize
/// - Strip NUL, BOM, most controls (keep `\n` only when `allow_newlines`)
/// - Strip bidi / zero-width controls
/// - Truncate to `max_chars` Unicode scalars
pub fn sanitize_display_string(input: &str, max_chars: usize, allow_newlines: bool) -> String {
    let nfc: String = input.nfc().collect();
    let mut out = String::with_capacity(nfc.len().min(max_chars.saturating_mul(4)));
    for ch in nfc.chars() {
        if should_strip(ch, allow_newlines) {
            continue;
        }
        out.push(ch);
    }
    out.chars().take(max_chars).collect()
}

/// Same as [`sanitize_display_string`], returning `default` when empty.
pub fn sanitize_display_string_or(
    input: &str,
    max_chars: usize,
    allow_newlines: bool,
    default: &str,
) -> String {
    let s = sanitize_display_string(input, max_chars, allow_newlines);
    if s.is_empty() {
        default.to_string()
    } else {
        s
    }
}

/// Sanitize a suggested Save As filename stem/name (SPEC §9.8).
///
/// Replaces `/ \ : * ? " < > |` and controls, blocks Windows reserved device
/// names, trims trailing dots/spaces, caps length. Extension is not added.
pub fn sanitize_filename(input: &str, max_len: usize) -> String {
    let max_len = max_len.clamp(1, 200);
    let mut out = String::with_capacity(input.len().min(max_len));
    for ch in input.chars() {
        if matches!(
            ch,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
        ) || ch.is_control()
        {
            out.push('_');
        } else {
            out.push(ch);
        }
        if out.chars().count() >= max_len {
            break;
        }
    }
    let trimmed: String = out
        .trim_end_matches(|c: char| c == '.' || c == ' ')
        .to_string();
    let stem = if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    };
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        format!("_{stem}")
    } else {
        stem
    }
}

fn should_strip(ch: char, allow_newlines: bool) -> bool {
    if ch == '\0' || ch == '\u{FEFF}' {
        return true;
    }
    if ch == '\n' {
        return !allow_newlines;
    }
    if ch == '\r' || ch == '\t' {
        return true;
    }
    if ch.is_control() {
        return true;
    }
    matches!(
        ch,
        '\u{200B}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{2060}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bidi_keeps_angle_brackets_as_data() {
        let raw = "hi\u{202E}TXT\u{202C}<script>";
        let s = sanitize_display_string(raw, 256, false);
        assert!(!s.contains('\u{202E}'));
        assert!(s.contains("<script>")); // data only — UI must still escape
        assert!(s.starts_with("hi"));
    }

    #[test]
    fn sanitize_filename_blocks_path_and_reserved() {
        assert_eq!(sanitize_filename("a/b\\c:d", 64), "a_b_c_d");
        assert_eq!(sanitize_filename("CON", 64), "_CON");
        assert_eq!(sanitize_filename("hello.", 64), "hello");
        assert_eq!(sanitize_filename("", 64), "untitled");
    }

    #[test]
    fn strips_nul_and_truncates() {
        let s = sanitize_display_string("a\0bcdefghij", 4, false);
        assert_eq!(s, "abcd");
    }

    #[test]
    fn empty_becomes_default() {
        let s = sanitize_display_string_or("\u{200B}\0", 16, false, "Untitled");
        assert_eq!(s, "Untitled");
    }

    #[test]
    fn keeps_newlines_in_description() {
        assert_eq!(sanitize_display_string("a\nb\rc", 64, true), "a\nbc");
        assert_eq!(sanitize_display_string("a\nb", 64, false), "ab");
    }
}
