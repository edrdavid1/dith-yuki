//! Create a top-down 32-bit HBITMAP from premultiplied BGRA.

use windows::core::Result as WinResult;
use windows::Win32::Foundation::E_OUTOFMEMORY;
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};

pub fn create_hbitmap_bgra_premul(width: i32, height: i32, bgra: &[u8]) -> WinResult<HBITMAP> {
    if width <= 0 || height <= 0 {
        return Err(windows::core::Error::from(E_OUTOFMEMORY));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| windows::core::Error::from(E_OUTOFMEMORY))?;
    if bgra.len() < expected {
        return Err(windows::core::Error::from(E_OUTOFMEMORY));
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default()],
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    // SAFETY: CreateDIBSection with a stack BITMAPINFO and null HDC is the documented
    // pattern for creating an HBITMAP we own until handed to the Shell.
    let hbmp = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)? };
    if hbmp.is_invalid() || bits.is_null() {
        return Err(windows::core::Error::from(E_OUTOFMEMORY));
    }

    // SAFETY: `bits` points at `expected` bytes of DIB storage for `hbmp`.
    unsafe {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, expected);
    }
    Ok(hbmp)
}

pub fn rgba_premul_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        out.push(px[2]);
        out.push(px[1]);
        out.push(px[0]);
        out.push(px[3]);
    }
    out
}

#[allow(dead_code)]
pub fn delete_bitmap(hbmp: HBITMAP) {
    // SAFETY: caller owns an HBITMAP that was not yet transferred to the Shell.
    unsafe {
        let _ = DeleteObject(hbmp.into());
    }
}
