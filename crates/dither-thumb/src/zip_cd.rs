//! Minimal ZIP central-directory parser over [`ReadAt`].

use crate::ThumbError;
use dither_zip_safe::io::{read_exact_at, ReadAt};
use dither_zip_safe::limits::ThumbLimits;

#[derive(Debug, Clone)]
pub struct ZipEntryMeta {
    pub name: String,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    pub encrypted: bool,
}

pub fn parse_central_directory(
    io: &mut dyn ReadAt,
    limits: &ThumbLimits,
    max_entries: u32,
) -> Result<Vec<ZipEntryMeta>, ThumbError> {
    let size = io.size();
    if size < 22 {
        return Err(ThumbError::Corrupt);
    }
    // Search EOCD in the last 64 KiB (+22).
    let search = size.min(65_535 + 22);
    let start = size - search;
    let mut tail = vec![0u8; search as usize];
    read_exact_at(io, start, &mut tail)?;

    let mut eocd_rel = None;
    for i in (0..=tail.len().saturating_sub(22)).rev() {
        if &tail[i..i + 4] == b"PK\x05\x06" {
            eocd_rel = Some(i);
            break;
        }
    }
    let eocd_rel = eocd_rel.ok_or(ThumbError::Corrupt)?;
    let eocd = &tail[eocd_rel..eocd_rel + 22];
    let cd_entries = u16::from_le_bytes([eocd[10], eocd[11]]) as u32;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as u64;
    let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as u64;

    // ZIP64 EOCD locator may precede EOCD — handle when classic fields are all-ones.
    let (cd_offset, cd_size, cd_entries) =
        if cd_offset == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF || cd_entries == 0xFFFF {
            parse_zip64(io, start + eocd_rel as u64, limits)?
        } else {
            (cd_offset, cd_size, cd_entries)
        };

    if cd_size > limits.max_central_directory {
        return Err(ThumbError::Limit);
    }
    if cd_entries > max_entries {
        return Err(ThumbError::Limit);
    }
    if cd_offset.saturating_add(cd_size) > size {
        return Err(ThumbError::Corrupt);
    }

    let mut cd = vec![0u8; cd_size as usize];
    read_exact_at(io, cd_offset, &mut cd)?;

    let mut entries = Vec::with_capacity(cd_entries as usize);
    let mut pos = 0usize;
    while pos + 46 <= cd.len() && (entries.len() as u32) < cd_entries {
        if &cd[pos..pos + 4] != b"PK\x01\x02" {
            return Err(ThumbError::Corrupt);
        }
        let flags = u16::from_le_bytes([cd[pos + 8], cd[pos + 9]]);
        let method = u16::from_le_bytes([cd[pos + 10], cd[pos + 11]]);
        let crc32 = u32::from_le_bytes([cd[pos + 16], cd[pos + 17], cd[pos + 18], cd[pos + 19]]);
        let mut comp =
            u32::from_le_bytes([cd[pos + 20], cd[pos + 21], cd[pos + 22], cd[pos + 23]]) as u64;
        let mut uncomp =
            u32::from_le_bytes([cd[pos + 24], cd[pos + 25], cd[pos + 26], cd[pos + 27]]) as u64;
        let name_len = u16::from_le_bytes([cd[pos + 28], cd[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([cd[pos + 30], cd[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([cd[pos + 32], cd[pos + 33]]) as usize;
        let mut local_off =
            u32::from_le_bytes([cd[pos + 42], cd[pos + 43], cd[pos + 44], cd[pos + 45]]) as u64;

        let name_start = pos + 46;
        let name_end = name_start + name_len;
        if name_end + extra_len + comment_len > cd.len() {
            return Err(ThumbError::Corrupt);
        }
        let name = std::str::from_utf8(&cd[name_start..name_end])
            .map_err(|_| ThumbError::Corrupt)?
            .to_string();

        // ZIP64 extra field (0x0001) when sizes are 0xFFFFFFFF.
        if comp == 0xFFFF_FFFF || uncomp == 0xFFFF_FFFF || local_off == 0xFFFF_FFFF {
            let extra = &cd[name_end..name_end + extra_len];
            let mut ep = 0usize;
            while ep + 4 <= extra.len() {
                let tag = u16::from_le_bytes([extra[ep], extra[ep + 1]]);
                let sz = u16::from_le_bytes([extra[ep + 2], extra[ep + 3]]) as usize;
                ep += 4;
                if ep + sz > extra.len() {
                    break;
                }
                if tag == 0x0001 {
                    let mut fp = ep;
                    if uncomp == 0xFFFF_FFFF {
                        if fp + 8 > ep + sz {
                            return Err(ThumbError::Corrupt);
                        }
                        uncomp = u64::from_le_bytes(extra[fp..fp + 8].try_into().unwrap());
                        fp += 8;
                    }
                    if comp == 0xFFFF_FFFF {
                        if fp + 8 > ep + sz {
                            return Err(ThumbError::Corrupt);
                        }
                        comp = u64::from_le_bytes(extra[fp..fp + 8].try_into().unwrap());
                        fp += 8;
                    }
                    if local_off == 0xFFFF_FFFF {
                        if fp + 8 > ep + sz {
                            return Err(ThumbError::Corrupt);
                        }
                        local_off = u64::from_le_bytes(extra[fp..fp + 8].try_into().unwrap());
                    }
                }
                ep += sz;
            }
        }

        entries.push(ZipEntryMeta {
            name,
            method,
            crc32,
            compressed_size: comp,
            uncompressed_size: uncomp,
            local_header_offset: local_off,
            encrypted: (flags & 1) != 0,
        });
        pos = name_end + extra_len + comment_len;
    }
    Ok(entries)
}

fn parse_zip64(
    io: &mut dyn ReadAt,
    eocd_abs: u64,
    limits: &ThumbLimits,
) -> Result<(u64, u64, u32), ThumbError> {
    // Locator is 20 bytes immediately before EOCD.
    if eocd_abs < 20 {
        return Err(ThumbError::Corrupt);
    }
    let mut loc = [0u8; 20];
    read_exact_at(io, eocd_abs - 20, &mut loc)?;
    if &loc[0..4] != b"PK\x06\x07" {
        return Err(ThumbError::Corrupt);
    }
    let zip64_eocd_off = u64::from_le_bytes(loc[8..16].try_into().unwrap());
    let mut hdr = [0u8; 56];
    read_exact_at(io, zip64_eocd_off, &mut hdr)?;
    if &hdr[0..4] != b"PK\x06\x06" {
        return Err(ThumbError::Corrupt);
    }
    let entries = u64::from_le_bytes(hdr[32..40].try_into().unwrap());
    let cd_size = u64::from_le_bytes(hdr[40..48].try_into().unwrap());
    let cd_offset = u64::from_le_bytes(hdr[48..56].try_into().unwrap());
    if entries > u32::MAX as u64 {
        return Err(ThumbError::Limit);
    }
    if cd_size > limits.max_central_directory {
        return Err(ThumbError::Limit);
    }
    Ok((cd_offset, cd_size, entries as u32))
}

pub fn find_entry<'a>(entries: &'a [ZipEntryMeta], name: &str) -> Option<&'a ZipEntryMeta> {
    entries.iter().find(|e| e.name == name)
}
