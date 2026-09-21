//! COM thumbnail provider implementing IInitializeWithStream + IThumbnailProvider.

use crate::bitmap::{create_hbitmap_bgra_premul, rgba_premul_to_bgra};
use crate::stream_io::StreamReadAt;
use dither_thumb::{extract, ThumbKind};
use dither_zip_safe::limits::ThumbLimits;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{Error, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{IStream, STATFLAG, STATSTG};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_ARGB, WTS_ALPHATYPE,
};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows_implement::implement;

pub static DLL_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

struct ProviderState {
    stream: Option<IStream>,
    size: u64,
}

#[implement(IThumbnailProvider, IInitializeWithStream)]
pub struct DitherThumbProvider {
    state: Mutex<ProviderState>,
}

impl DitherThumbProvider {
    pub fn new() -> Self {
        DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self {
            state: Mutex::new(ProviderState {
                stream: None,
                size: 0,
            }),
        }
    }

    fn take_stream(&self) -> WinResult<(IStream, u64)> {
        let guard = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        let stream = guard.stream.clone().ok_or_else(|| Error::from(E_FAIL))?;
        Ok((stream, guard.size))
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
        pstream: windows::core::Ref<'_, IStream>,
        _grf_mode: u32,
    ) -> WinResult<()> {
        let stream = pstream.ok()?.clone();
        let size = unsafe {
            let mut stat = STATSTG::default();
            stream.Stat(&mut stat, STATFLAG(0))?;
            stat.cbSize
        };
        if size == 0 {
            return Err(Error::from(E_INVALIDARG));
        }
        let mut guard = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        guard.stream = Some(stream);
        guard.size = size;
        Ok(())
    }
}

impl IThumbnailProvider_Impl for DitherThumbProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdw_alpha: *mut WTS_ALPHATYPE,
    ) -> WinResult<()> {
        if phbmp.is_null() || pdw_alpha.is_null() {
            return Err(Error::from(E_POINTER));
        }
        unsafe {
            *phbmp = HBITMAP::default();
            *pdw_alpha = WTSAT_ARGB;
        }

        let (stream, size) = self.take_stream()?;
        let limits = ThumbLimits::default();
        let deadline = Instant::now() + Duration::from_millis(limits.deadline_ms);
        let max_side = cx.min(1024).max(1);

        let bmp = {
            let mut io = StreamReadAt::new(
                stream.clone(),
                size,
                limits.max_read_at_ops,
                deadline,
            );
            match extract(&mut io, ThumbKind::Project, max_side, true, &limits) {
                Ok(b) => b,
                Err(_) => {
                    let mut io2 =
                        StreamReadAt::new(stream, size, limits.max_read_at_ops, deadline);
                    extract(&mut io2, ThumbKind::Pattern, max_side, true, &limits)
                        .map_err(|_| Error::from(E_FAIL))?
                }
            }
        };

        let bgra = rgba_premul_to_bgra(&bmp.rgba);
        let hbmp = create_hbitmap_bgra_premul(bmp.width as i32, bmp.height as i32, &bgra)?;

        unsafe {
            *phbmp = hbmp;
            *pdw_alpha = WTSAT_ARGB;
        }
        Ok(())
    }
}
