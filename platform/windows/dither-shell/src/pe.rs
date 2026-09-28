//! Enough of a PE header to compare a DLL's machine type with the OS.

pub const MACHINE_AMD64: u16 = 0x8664;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MACHINE_ARM64: u16 = 0xAA64;

pub fn pe_machine(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return None;
    }
    let pe_off = u32::from_le_bytes(bytes[0x3c..0x40].try_into().ok()?) as usize;
    if bytes.len() < pe_off + 6 || &bytes[pe_off..pe_off + 4] != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(
        bytes[pe_off + 4..pe_off + 6].try_into().ok()?,
    ))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn machine_name(machine: u16) -> &'static str {
    match machine {
        MACHINE_AMD64 => "x64",
        MACHINE_ARM64 => "arm64",
        0x14c => "x86",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_amd64_machine() {
        let mut bytes = vec![0u8; 0x80];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c] = 0x40;
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44] = 0x64;
        bytes[0x45] = 0x86;
        assert_eq!(pe_machine(&bytes), Some(MACHINE_AMD64));
    }
}
