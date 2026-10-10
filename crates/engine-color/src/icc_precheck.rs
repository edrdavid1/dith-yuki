//! Cheap structural ICC checks before handing bytes to moxcms.
//!
//! A timeout on `spawn_blocking` only abandons the waiter — the worker thread
//! keeps running. Rejecting obviously hostile/truncated profiles here cuts most
//! DoS surface without needing a killable subprocess (still the stronger option
//! for untrusted import at scale).

use crate::soft_proof::SoftProofError;

/// ICC profile header size (ICC.1:2010).
const ICC_HEADER_LEN: usize = 128;
const MAX_TAG_COUNT: u32 = 1024;
const MAX_ICC_BYTES: usize = 8 * 1024 * 1024;

/// Validate header size field, tag table bounds, and that each tag lies in-file.
pub fn precheck_icc_bytes(bytes: &[u8]) -> Result<(), SoftProofError> {
    if bytes.is_empty() {
        return Err(SoftProofError::Parse("empty ICC".into()));
    }
    if bytes.len() > MAX_ICC_BYTES {
        return Err(SoftProofError::Parse(format!(
            "ICC too large ({} > {MAX_ICC_BYTES})",
            bytes.len()
        )));
    }
    if bytes.len() < ICC_HEADER_LEN {
        return Err(SoftProofError::Parse(format!(
            "ICC truncated ({} < {ICC_HEADER_LEN} header bytes)",
            bytes.len()
        )));
    }

    let declared = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
    // Allow declared size == file size; reject claims past EOF or tiny headers.
    if declared < ICC_HEADER_LEN || declared > bytes.len() {
        return Err(SoftProofError::Parse(format!(
            "ICC size field {declared} inconsistent with {} bytes on disk",
            bytes.len()
        )));
    }

    // Color space / class fourcc at 16..20 and 12..16 — ASCII letters/digits or
    // trailing spaces (ICC pads short signatures with `0x20`).
    let is_fourcc = |b: &u8| b.is_ascii_alphanumeric() || *b == b' ';
    let color_space = &bytes[16..20];
    if !color_space.iter().all(is_fourcc) {
        return Err(SoftProofError::Parse(
            "ICC color space signature is not a valid fourcc".into(),
        ));
    }
    let class = &bytes[12..16];
    if !class.iter().all(is_fourcc) {
        return Err(SoftProofError::Parse(
            "ICC profile class signature is not a valid fourcc".into(),
        ));
    }

    // Tag count at offset 128.
    if bytes.len() < ICC_HEADER_LEN + 4 {
        return Err(SoftProofError::Parse(
            "ICC missing tag count after header".into(),
        ));
    }
    let tag_count = u32::from_be_bytes(bytes[128..132].try_into().unwrap());
    if tag_count > MAX_TAG_COUNT {
        return Err(SoftProofError::Parse(format!(
            "ICC tag count {tag_count} exceeds {MAX_TAG_COUNT}"
        )));
    }
    let tag_table_end = ICC_HEADER_LEN
        .saturating_add(4)
        .saturating_add((tag_count as usize).saturating_mul(12));
    if tag_table_end > bytes.len() {
        return Err(SoftProofError::Parse(format!(
            "ICC tag table extends to {tag_table_end} past {} bytes",
            bytes.len()
        )));
    }

    let mut off = ICC_HEADER_LEN + 4;
    for i in 0..tag_count as usize {
        let entry = &bytes[off..off + 12];
        let tag_offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
        let tag_size = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
        if tag_size == 0 {
            return Err(SoftProofError::Parse(format!("ICC tag[{i}] has zero size")));
        }
        let end = tag_offset.saturating_add(tag_size);
        if tag_offset < ICC_HEADER_LEN || end > bytes.len() {
            return Err(SoftProofError::Parse(format!(
                "ICC tag[{i}] range {tag_offset}..{end} outside file ({})",
                bytes.len()
            )));
        }
        // Reject absurd single-tag allocations (compressed bomb style).
        if tag_size > MAX_ICC_BYTES / 2 {
            return Err(SoftProofError::Parse(format!(
                "ICC tag[{i}] size {tag_size} implausibly large"
            )));
        }
        off += 12;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_proof::BUILTIN_FOGRA51_ICC;

    #[test]
    fn real_fogra51_passes_precheck() {
        precheck_icc_bytes(BUILTIN_FOGRA51_ICC).expect("FOGRA51");
    }

    #[test]
    fn empty_and_tiny_fail() {
        assert!(precheck_icc_bytes(&[]).is_err());
        assert!(precheck_icc_bytes(&[0u8; 64]).is_err());
    }

    #[test]
    fn bogus_size_field_fails() {
        let mut b = BUILTIN_FOGRA51_ICC.to_vec();
        b[0..4].copy_from_slice(&u32::to_be_bytes(u32::MAX));
        assert!(precheck_icc_bytes(&b).is_err());
    }

    #[test]
    fn truncated_tag_table_fails() {
        let mut b = BUILTIN_FOGRA51_ICC[..200].to_vec();
        // Claim many tags.
        b.resize(132, 0);
        b[128..132].copy_from_slice(&u32::to_be_bytes(50));
        assert!(precheck_icc_bytes(&b).is_err());
    }
}
