//! Failure reasons for the shell providers.
//!
//! Codes only: no paths, names, or file bytes. The in-process ring is what
//! `dither-shell-diag last-errors` reads after `render` in the same process.
//! On Windows the same codes are also published in a session mapping so a
//! later `last-errors` can see failures from `dllhost.exe` / `prevhost.exe`.

use std::sync::Mutex;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NotOurFile = 1,
    NoThumbnail = 2,
    Limit = 3,
    Corrupt = 4,
    Timeout = 5,
    BitmapCreateFailed = 6,
    ArchMismatch = 7,
    InitFailed = 8,
    AlreadyInitialized = 9,
    InvalidArg = 10,
    Internal = 11,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::NotOurFile => "NOT_OUR_FILE",
            Reason::NoThumbnail => "NO_THUMBNAIL",
            Reason::Limit => "LIMIT",
            Reason::Corrupt => "CORRUPT",
            Reason::Timeout => "TIMEOUT",
            Reason::BitmapCreateFailed => "BITMAP_CREATE_FAILED",
            Reason::ArchMismatch => "ARCH_MISMATCH",
            Reason::InitFailed => "INIT_FAILED",
            Reason::AlreadyInitialized => "ALREADY_INITIALIZED",
            Reason::InvalidArg => "INVALID_ARG",
            Reason::Internal => "INTERNAL",
        }
    }

    pub fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            1 => Reason::NotOurFile,
            2 => Reason::NoThumbnail,
            3 => Reason::Limit,
            4 => Reason::Corrupt,
            5 => Reason::Timeout,
            6 => Reason::BitmapCreateFailed,
            7 => Reason::ArchMismatch,
            8 => Reason::InitFailed,
            9 => Reason::AlreadyInitialized,
            10 => Reason::InvalidArg,
            11 => Reason::Internal,
            _ => return None,
        })
    }
}

const CAP: usize = 32;

struct Ring {
    seq: u32,
    codes: [u32; CAP],
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    seq: 0,
    codes: [0; CAP],
});

pub fn record(reason: Reason) {
    if let Ok(mut ring) = RING.lock() {
        let i = (ring.seq as usize) % CAP;
        ring.codes[i] = reason as u32;
        ring.seq = ring.seq.wrapping_add(1);
    }
    #[cfg(windows)]
    publish_shared(reason as u32);
}

pub fn recent() -> Vec<Reason> {
    let Ok(ring) = RING.lock() else {
        return Vec::new();
    };
    let n = (ring.seq as usize).min(CAP);
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let idx = (ring.seq as usize - 1 - k) % CAP;
        if let Some(r) = Reason::from_code(ring.codes[idx]) {
            out.push(r);
        }
    }
    out
}

pub fn recent_lines() -> String {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut lines: Vec<String> = recent()
        .into_iter()
        .map(|r| r.as_str().to_string())
        .collect();
    #[cfg(windows)]
    {
        if lines.is_empty() {
            lines = read_shared()
                .into_iter()
                .map(|r| r.as_str().to_string())
                .collect();
        }
    }
    if lines.is_empty() {
        "(none)".into()
    } else {
        lines.join("\n")
    }
}

#[cfg(windows)]
fn mapping() -> Option<*mut u8> {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{
        CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    };
    static VIEW: OnceLock<usize> = OnceLock::new();
    let addr = VIEW.get_or_init(|| {
        let name: Vec<u16> = "Local\\DitherShellDiagCodes"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: named pagefile mapping, fixed size, codes only.
        unsafe {
            let handle = CreateFileMappingW(
                windows::Win32::Foundation::INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                (8 + CAP * 4) as u32,
                windows::core::PCWSTR(name.as_ptr()),
            );
            let Ok(handle) = handle else {
                return 0;
            };
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 8 + CAP * 4);
            // The view holds the mapping; the handle can close.
            let _ = CloseHandle(handle);
            view.Value as usize
        }
    });
    if *addr == 0 {
        None
    } else {
        Some(*addr as *mut u8)
    }
}

#[cfg(windows)]
fn publish_shared(code: u32) {
    let Some(ptr) = mapping() else {
        return;
    };
    // SAFETY: mapping is 8 + CAP*4 bytes, written as a seqlock-free ring of codes.
    unsafe {
        let seq = ptr as *mut u32;
        let n = seq.read();
        let slot = (n as usize) % CAP;
        let codes = ptr.add(8) as *mut u32;
        codes.add(slot).write(code);
        seq.write(n.wrapping_add(1));
    }
}

#[cfg(windows)]
fn read_shared() -> Vec<Reason> {
    let Some(ptr) = mapping() else {
        return Vec::new();
    };
    // SAFETY: same layout as `publish_shared`.
    unsafe {
        let seq = *(ptr as *const u32);
        let n = (seq as usize).min(CAP);
        let codes = ptr.add(8) as *const u32;
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let idx = (seq as usize - 1 - k) % CAP;
            if let Some(r) = Reason::from_code(codes.add(idx).read()) {
                out.push(r);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_newest_first() {
        record(Reason::Limit);
        record(Reason::Timeout);
        let got = recent();
        assert_eq!(got.first().copied(), Some(Reason::Timeout));
        assert!(got.contains(&Reason::Limit));
    }
}
