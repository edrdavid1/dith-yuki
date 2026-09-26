//! C ABI for OS preview providers (preview SPEC §5.2).
//!
//! `unsafe` only here; every block has a `// SAFETY:` comment.
//! Exported functions catch panics → `DT_INTERNAL`.

use dither_thumb::{extract, ThumbError, ThumbKind};
use dither_zip_safe::io::{IoError, ReadAt};
use dither_zip_safe::limits::ThumbLimits;
use std::os::raw::{c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

pub const DT_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DtStatus {
    Ok = 0,
    NotAvailable = 1,
    Unsupported = 2,
    Limit = 3,
    Corrupt = 4,
    Timeout = 5,
    Internal = 6,
    BadArg = 7,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum DtKind {
    Project = 1,
    Pattern = 2,
}

#[repr(C)]
pub struct DtIo {
    pub ctx: *mut c_void,
    pub size: u64,
    pub read_at: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            offset: u64,
            buf: *mut u8,
            len: usize,
            out_read: *mut usize,
        ) -> c_int,
    >,
}

#[repr(C)]
pub struct DtBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: *mut u8,
    pub len: usize,
}

struct FfiReadAt {
    io: DtIo,
    ops: u32,
    max_ops: u32,
    deadline: Instant,
}

impl ReadAt for FfiReadAt {
    fn size(&self) -> u64 {
        self.io.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        if Instant::now() >= self.deadline {
            return Err(IoError::Timeout);
        }
        if self.ops >= self.max_ops {
            return Err(IoError::TooManyOps);
        }
        self.ops = self.ops.saturating_add(1);
        let Some(cb) = self.io.read_at else {
            return Err(IoError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "null read_at",
            )));
        };
        let mut out_read = 0usize;
        // SAFETY: caller-provided callback; buf is valid for buf.len().
        let rc = unsafe {
            cb(
                self.io.ctx,
                offset,
                buf.as_mut_ptr(),
                buf.len(),
                &mut out_read,
            )
        };
        if rc != 0 {
            return Err(IoError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "read_at failed",
            )));
        }
        if out_read > buf.len() {
            return Err(IoError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "out_read > len",
            )));
        }
        Ok(out_read)
    }
}

fn map_err(e: ThumbError) -> DtStatus {
    match e {
        ThumbError::NotAvailable => DtStatus::NotAvailable,
        ThumbError::Unsupported => DtStatus::Unsupported,
        ThumbError::Limit => DtStatus::Limit,
        ThumbError::Corrupt => DtStatus::Corrupt,
        ThumbError::Timeout => DtStatus::Timeout,
        ThumbError::BadArg => DtStatus::BadArg,
        ThumbError::Internal => DtStatus::Internal,
    }
}

fn kind_from(k: DtKind) -> Option<ThumbKind> {
    match k {
        DtKind::Project => Some(ThumbKind::Project),
        DtKind::Pattern => Some(ThumbKind::Pattern),
    }
}

#[no_mangle]
pub extern "C" fn dt_abi_version() -> u32 {
    DT_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn dt_extract(
    io: *const DtIo,
    kind: DtKind,
    max_side: u32,
    premultiply: c_int,
    out: *mut DtBitmap,
) -> DtStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if io.is_null() || out.is_null() {
            return DtStatus::BadArg;
        }
        // SAFETY: io/out non-null; caller owns them for the duration of the call.
        let io_ref = unsafe { &*io };
        if io_ref.read_at.is_none() || io_ref.size == 0 {
            return DtStatus::BadArg;
        }
        let Some(thumb_kind) = kind_from(kind) else {
            return DtStatus::BadArg;
        };
        let limits = ThumbLimits::default();
        let deadline = Instant::now() + Duration::from_millis(limits.deadline_ms);
        let mut reader = FfiReadAt {
            io: DtIo {
                ctx: io_ref.ctx,
                size: io_ref.size,
                read_at: io_ref.read_at,
            },
            ops: 0,
            max_ops: limits.max_read_at_ops,
            deadline,
        };
        match extract(&mut reader, thumb_kind, max_side, premultiply != 0, &limits) {
            Ok(bmp) => {
                let width = bmp.width;
                let height = bmp.height;
                let boxed = bmp.rgba.into_boxed_slice();
                let len = boxed.len();
                let raw = Box::into_raw(boxed);
                // SAFETY: out is non-null and points to a DtBitmap we may write.
                unsafe {
                    (*out).width = width;
                    (*out).height = height;
                    (*out).rgba = raw as *mut u8;
                    (*out).len = len;
                }
                DtStatus::Ok
            }
            Err(e) => map_err(e),
        }
    }));
    match result {
        Ok(status) => status,
        Err(_) => DtStatus::Internal,
    }
}

#[no_mangle]
pub extern "C" fn dt_free_bitmap(bmp: *mut DtBitmap) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if bmp.is_null() {
            return;
        }
        // SAFETY: bmp non-null; rgba is either null or from Box::into_raw in dt_extract.
        unsafe {
            if (*bmp).rgba.is_null() || (*bmp).len == 0 {
                (*bmp).rgba = std::ptr::null_mut();
                (*bmp).len = 0;
                (*bmp).width = 0;
                (*bmp).height = 0;
                return;
            }
            let raw = std::ptr::slice_from_raw_parts_mut((*bmp).rgba, (*bmp).len);
            drop(Box::from_raw(raw));
            (*bmp).rgba = std::ptr::null_mut();
            (*bmp).len = 0;
            (*bmp).width = 0;
            (*bmp).height = 0;
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    struct MemCtx {
        data: Vec<u8>,
    }

    unsafe extern "C" fn mem_read_at(
        ctx: *mut c_void,
        offset: u64,
        buf: *mut u8,
        len: usize,
        out_read: *mut usize,
    ) -> c_int {
        let ctx = &*(ctx as *const MemCtx);
        if offset as usize > ctx.data.len() {
            return 1;
        }
        let start = offset as usize;
        let n = (ctx.data.len() - start).min(len);
        std::ptr::copy_nonoverlapping(ctx.data.as_ptr().add(start), buf, n);
        *out_read = n;
        0
    }

    fn sample_zip() -> Vec<u8> {
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255])
                .unwrap();
        }
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("mimetype", stored).unwrap();
            zip.write_all(b"application/vnd.dither.project+zip")
                .unwrap();
            zip.start_file("thumbnail.png", stored).unwrap();
            zip.write_all(&png).unwrap();
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn ffi_extract_and_free() {
        let data = sample_zip();
        let mut ctx = MemCtx { data };
        let io = DtIo {
            ctx: &mut ctx as *mut MemCtx as *mut c_void,
            size: ctx.data.len() as u64,
            read_at: Some(mem_read_at),
        };
        let mut out = DtBitmap {
            width: 0,
            height: 0,
            rgba: std::ptr::null_mut(),
            len: 0,
        };
        let st = dt_extract(&io, DtKind::Project, 64, 0, &mut out);
        assert_eq!(st as i32, DtStatus::Ok as i32);
        assert_eq!(out.width, 2);
        assert!(!out.rgba.is_null());
        dt_free_bitmap(&mut out);
        assert!(out.rgba.is_null());
        dt_free_bitmap(&mut out); // idempotent
        dt_free_bitmap(std::ptr::null_mut());
    }

    #[test]
    fn abi_version_nonzero() {
        assert_eq!(dt_abi_version(), 1);
    }
}
