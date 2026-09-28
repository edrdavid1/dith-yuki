//! Top-down 32 bpp DIB. The bitmap is `width × height` from `dt_extract`,
//! never `cx × cx`.

use crate::color::bgra_to_rgba;
use windows::core::{Error, Result as WinResult};
use windows::Win32::Foundation::E_OUTOFMEMORY;
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};

pub fn create_bgra_premul_dib(width: i32, height: i32, bgra: &[u8]) -> WinResult<HBITMAP> {
    if width <= 0 || height <= 0 {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::from(E_OUTOFMEMORY))?;
    if bgra.len() < expected {
        return Err(Error::from(E_OUTOFMEMORY));
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
    // SAFETY: stack BITMAPINFO, null HDC. The returned HBITMAP owns `bits`
    // until DeleteObject or until the Shell takes the thumbnail bitmap.
    let hbmp = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)? };
    if hbmp.is_invalid() || bits.is_null() {
        if !hbmp.is_invalid() {
            delete_bitmap(hbmp);
        }
        return Err(Error::from(E_OUTOFMEMORY));
    }
    // SAFETY: `bits` addresses `expected` bytes of the DIB just created.
    unsafe {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, expected);
    }
    Ok(hbmp)
}

pub fn delete_bitmap(hbmp: HBITMAP) {
    if hbmp.is_invalid() {
        return;
    }
    // SAFETY: caller still owns this HBITMAP.
    unsafe {
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
    }
}

pub fn bitmap_size(hbmp: HBITMAP) -> WinResult<(i32, i32)> {
    let mut bm = BITMAP::default();
    // SAFETY: BITMAP is the documented buffer for GetObject on an HBITMAP.
    let n = unsafe {
        GetObjectW(
            HGDIOBJ(hbmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bm) as *mut BITMAP as *mut _),
        )
    };
    if n == 0 {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    Ok((bm.bmWidth, bm.bmHeight.abs()))
}

/// Read the DIB back as premultiplied RGBA (top-down).
pub fn read_premul_rgba(hbmp: HBITMAP) -> WinResult<(u32, u32, Vec<u8>)> {
    let (w, h) = bitmap_size(hbmp)?;
    if w <= 0 || h <= 0 {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
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
    let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
    // SAFETY: screen DC is released below; `bgra` matches the BITMAPINFO.
    let lines = unsafe {
        let hdc = GetDC(None);
        let lines = GetDIBits(
            hdc,
            hbmp,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        lines
    };
    if lines == 0 {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    Ok((w as u32, h as u32, bgra_to_rgba(&bgra)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS};

    #[test]
    fn dib_size_matches_the_buffer_not_a_square() {
        let w = 8i32;
        let h = 5i32;
        let bgra = vec![255u8; (w * h * 4) as usize];
        let hbmp = create_bgra_premul_dib(w, h, &bgra).unwrap();
        let (rw, rh) = bitmap_size(hbmp).unwrap();
        assert_eq!((rw, rh), (w, h));
        let (pw, ph, px) = read_premul_rgba(hbmp).unwrap();
        assert_eq!((pw, ph, px.len()), (8, 5, 8 * 5 * 4));
        delete_bitmap(hbmp);
    }

    #[test]
    fn rejected_sizes_do_not_grow_the_gdi_count() {
        let before = gdi_count();
        for _ in 0..64 {
            assert!(create_bgra_premul_dib(0, 10, &[0, 0, 0, 0]).is_err());
            assert!(create_bgra_premul_dib(4, 4, &[0, 0, 0]).is_err());
        }
        let hbmp = create_bgra_premul_dib(
            2,
            2,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        )
        .unwrap();
        delete_bitmap(hbmp);
        let after = gdi_count();
        assert_eq!(before, after);
    }

    fn gdi_count() -> u32 {
        unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) }
    }
}
