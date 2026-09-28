//! `IThumbnailProvider` + `IInitializeWithStream`.

use crate::color::rgba_premul_to_bgra;
use crate::diag_codes::{record, Reason};
use crate::extract::extract_preview;
use crate::gdi::create_bgra_premul_dib;
use crate::guard::catch_com;
use crate::stream_io::{BoundStream, StreamReadAt};
use dither_zip_safe::limits::ThumbLimits;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{Error, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_ARGB, WTS_ALPHATYPE, WTS_E_FAILEDEXTRACTION,
};
use windows_implement::implement;

pub static DLL_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

#[implement(IThumbnailProvider, IInitializeWithStream)]
pub struct DitherThumbProvider {
    state: Mutex<BoundStream>,
}

impl DitherThumbProvider {
    pub fn new() -> Self {
        DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self {
            state: Mutex::new(BoundStream::new()),
        }
    }
}

impl Drop for DitherThumbProvider {
    fn drop(&mut self) {
        DLL_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IInitializeWithStream_Impl for DitherThumbProvider_Impl {
    fn Initialize(
        &self,
        pstream: windows::core::Ref<'_, windows::Win32::System::Com::IStream>,
        _grf_mode: u32,
    ) -> WinResult<()> {
        catch_com(|| {
            let mut guard = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            guard.initialize(pstream)
        })
    }
}

impl IThumbnailProvider_Impl for DitherThumbProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdw_alpha: *mut WTS_ALPHATYPE,
    ) -> WinResult<()> {
        catch_com(|| get_thumbnail(self, cx, phbmp, pdw_alpha))
    }
}

fn get_thumbnail(
    provider: &DitherThumbProvider_Impl,
    cx: u32,
    phbmp: *mut HBITMAP,
    pdw_alpha: *mut WTS_ALPHATYPE,
) -> WinResult<()> {
    if phbmp.is_null() || pdw_alpha.is_null() {
        record(Reason::InvalidArg);
        return Err(Error::from(E_POINTER));
    }
    unsafe {
        *phbmp = HBITMAP::default();
        *pdw_alpha = WTS_ALPHATYPE(0);
    }
    if cx == 0 {
        record(Reason::InvalidArg);
        return Err(Error::from(E_INVALIDARG));
    }

    let (stream, size) = {
        let guard = provider.state.lock().map_err(|_| Error::from(E_FAIL))?;
        let stream = guard.stream.clone().ok_or_else(|| {
            record(Reason::InitFailed);
            Error::from(E_FAIL)
        })?;
        (stream, guard.size)
    };

    let limits = ThumbLimits::default();
    let deadline = Instant::now() + Duration::from_millis(limits.deadline_ms);
    let max_side = cx.min(1024);
    let mut io = StreamReadAt::new(stream, size, limits.max_read_at_ops, deadline);
    let bmp = extract_preview(&mut io, max_side, true, &limits).map_err(|reason| {
        record(reason);
        Error::from(WTS_E_FAILEDEXTRACTION)
    })?;

    let bgra = rgba_premul_to_bgra(&bmp.rgba);
    let hbmp = create_bgra_premul_dib(bmp.width as i32, bmp.height as i32, &bgra).map_err(|e| {
        record(Reason::BitmapCreateFailed);
        e
    })?;

    unsafe {
        *phbmp = hbmp;
        *pdw_alpha = WTSAT_ARGB;
    }
    Ok(())
}
