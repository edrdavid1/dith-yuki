//! Entry-name allowlist and syntax checks (format SPEC §6.2).

use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

pub const MAX_PATH_SEGMENTS: u32 = 4;
pub const MAX_ENTRY_NAME_BYTES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryClass {
    Allowed,
    Unknown,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EntrySyntaxError {
    #[error("unsafe entry name: {0}")]
    Unsafe(String),
}

pub fn validate_entry_syntax(name: &str) -> Result<(), EntrySyntaxError> {
    if name.is_empty() || name.len() > MAX_ENTRY_NAME_BYTES {
        return Err(EntrySyntaxError::Unsafe(name.into()));
    }
    if name.contains('\\') || name.contains('\0') {
        return Err(EntrySyntaxError::Unsafe(name.into()));
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(EntrySyntaxError::Unsafe(name.into()));
    }
    let segments: Vec<&str> = name.split('/').collect();
    if segments.len() as u32 > MAX_PATH_SEGMENTS {
        return Err(EntrySyntaxError::Unsafe(name.into()));
    }
    for seg in &segments {
        if seg.is_empty() || *seg == "." || *seg == ".." {
            return Err(EntrySyntaxError::Unsafe(name.into()));
        }
        if seg.ends_with(' ') || seg.ends_with('.') {
            return Err(EntrySyntaxError::Unsafe(name.into()));
        }
    }
    let nfc: String = name.nfc().collect();
    if nfc != name {
        return Err(EntrySyntaxError::Unsafe(name.into()));
    }
    Ok(())
}

pub fn is_allowlisted_entry(name: &str) -> bool {
    matches!(
        name,
        "mimetype"
            | "manifest.json"
            | "document.json"
            | "filters.json"
            | "palettes.json"
            | "composite.png"
            | "thumbnail.png"
    ) || is_layer_png(name)
        || is_threshold_map(name)
        || is_ext_entry(name)
}

fn is_layer_png(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("layers/") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".png") else {
        return false;
    };
    is_id_token(stem)
}

fn is_threshold_map(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("assets/threshold_maps/") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".png") else {
        return false;
    };
    let len = stem.len();
    (len == 32 || len == 64) && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_ext_entry(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ext/") else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.is_empty() || segments.len() > 3 {
        return false;
    }
    segments.iter().all(|s| is_ext_token(s))
}

fn is_id_token(s: &str) -> bool {
    let len = s.len();
    (1..=64).contains(&len) && s.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

fn is_ext_token(s: &str) -> bool {
    let len = s.len();
    (1..=64).contains(&len)
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

pub fn classify_entry_name(name: &str) -> EntryClass {
    if is_allowlisted_entry(name) {
        EntryClass::Allowed
    } else {
        EntryClass::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_core_names() {
        assert!(is_allowlisted_entry("thumbnail.png"));
        assert!(is_allowlisted_entry("mimetype"));
        assert!(!is_allowlisted_entry("../thumbnail.png"));
    }

    #[test]
    fn rejects_traversal() {
        assert!(validate_entry_syntax("../x").is_err());
        assert!(validate_entry_syntax("a/../../b").is_err());
    }
}
