//! Explorer preview pane (`IPreviewHandler` and the interfaces it requires).
//!
//! The bitmap size still comes from `dt_extract`. This module only centers
//! that bitmap in the host window. DPI is already reflected in the host
//! rectangle, so it is not applied again.

use crate::color::rgba_premul_to_bgra;
use crate::diag_codes::{record, Reason};
use crate::extract::extract_preview;
use crate::gdi::{create_bgra_premul_dib, delete_bitmap};
use crate::geometry::fit_centered;
use crate::guard::catch_com;
use crate::stream_io::{BoundStream, StreamReadAt};
use crate::thumbnail::DLL_LOCK_COUNT;
use dither_zip_safe::limits::ThumbLimits;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{Error, IUnknown, Interface, Result as WinResult, GUID};
use windows::Win32::Foundation::{
    COLORREF, E_FAIL, E_INVALIDARG, E_POINTER, HWND, LPARAM, LRESULT, RECT, S_FALSE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, CreateCompatibleDC, CreateFontIndirectW, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, InvalidateRect, ReleaseDC, SelectObject,
    SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, DT_CENTER, DT_SINGLELINE,
    DT_VCENTER, HBITMAP, HDC, HGDIOBJ, LOGFONTW, TRANSPARENT,
};
use windows::Win32::System::Com::IStream;
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Ole::{
    IObjectWithSite, IObjectWithSite_Impl, IOleWindow, IOleWindow_Impl,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{
    IPreviewHandler, IPreviewHandlerVisuals, IPreviewHandlerVisuals_Impl, IPreviewHandler_Impl,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, MoveWindow,
    PostQuitMessage, RegisterClassW, SetWindowLongPtrW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HMENU, WINDOW_EX_STYLE, WM_DESTROY, WM_ERASEBKGND, WM_NCCREATE, WM_PAINT,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows_core::BOOL;
use windows_implement::implement;

struct PreviewState {
    bound: BoundStream,
    parent: HWND,
    child: HWND,
    rect: RECT,
    hbmp: HBITMAP,
    width: u32,
    height: u32,
    bg: COLORREF,
    text: COLORREF,
    font: LOGFONTW,
    has_font: bool,
    site: Option<IUnknown>,
    failed: bool,
}

#[implement(
    IPreviewHandler,
    IInitializeWithStream,
    IObjectWithSite,
    IOleWindow,
    IPreviewHandlerVisuals
)]
pub struct DitherPreviewHandler {
    state: Mutex<PreviewState>,
}

impl DitherPreviewHandler {
    pub fn new() -> Self {
        DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self {
            state: Mutex::new(PreviewState {
                bound: BoundStream::new(),
                parent: HWND::default(),
                child: HWND::default(),
                rect: RECT::default(),
                hbmp: HBITMAP::default(),
                width: 0,
                height: 0,
                bg: COLORREF(0x00FF_FFFF),
                text: COLORREF(0),
                font: LOGFONTW::default(),
                has_font: false,
                site: None,
                failed: false,
            }),
        }
    }
}

impl Drop for DitherPreviewHandler {
    fn drop(&mut self) {
        release_preview(&self.state);
        DLL_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

fn release_preview(state: &Mutex<PreviewState>) {
    let Ok(mut g) = state.lock() else {
        return;
    };
    if !g.child.is_invalid() {
        unsafe {
            let _ = DestroyWindow(g.child);
        }
        g.child = HWND::default();
    }
    if !g.hbmp.is_invalid() {
        delete_bitmap(g.hbmp);
        g.hbmp = HBITMAP::default();
    }
    g.bound.stream = None;
    g.bound.size = 0;
}

impl IInitializeWithStream_Impl for DitherPreviewHandler_Impl {
    fn Initialize(
        &self,
        pstream: windows::core::Ref<'_, IStream>,
        _grf_mode: u32,
    ) -> WinResult<()> {
        catch_com(|| {
            let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            g.bound.initialize(pstream)
        })
    }
}

impl IPreviewHandler_Impl for DitherPreviewHandler_Impl {
    fn SetWindow(&self, hwnd: HWND, prc: *const RECT) -> WinResult<()> {
        catch_com(|| {
            if hwnd.is_invalid() || prc.is_null() {
                record(Reason::InvalidArg);
                return Err(Error::from(E_INVALIDARG));
            }
            let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            g.parent = hwnd;
            g.rect = unsafe { *prc };
            let child = g.child;
            let rect = g.rect;
            drop(g);
            place_child(child, rect);
            Ok(())
        })
    }

    fn SetRect(&self, prc: *const RECT) -> WinResult<()> {
        catch_com(|| {
            if prc.is_null() {
                record(Reason::InvalidArg);
                return Err(Error::from(E_INVALIDARG));
            }
            let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            g.rect = unsafe { *prc };
            let child = g.child;
            let rect = g.rect;
            drop(g);
            place_child(child, rect);
            Ok(())
        })
    }

    fn DoPreview(&self) -> WinResult<()> {
        catch_com(|| do_preview(self))
    }

    fn Unload(&self) -> WinResult<()> {
        catch_com(|| {
            release_preview(&self.state);
            Ok(())
        })
    }

    fn SetFocus(&self) -> WinResult<()> {
        catch_com(|| {
            let g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            let hwnd = if g.child.is_invalid() {
                g.parent
            } else {
                g.child
            };
            drop(g);
            if !hwnd.is_invalid() {
                unsafe {
                    let _ = SetFocus(Some(hwnd));
                }
            }
            Ok(())
        })
    }

    fn QueryFocus(&self) -> WinResult<HWND> {
        catch_com(|| {
            let focus = unsafe { GetFocus() };
            if !focus.is_invalid() {
                return Ok(focus);
            }
            let g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
            if !g.child.is_invalid() {
                Ok(g.child)
            } else {
                record(Reason::InitFailed);
                Err(Error::from(E_FAIL))
            }
        })
    }

    fn TranslateAccelerator(
        &self,
        _pmsg: *const windows::Win32::UI::WindowsAndMessaging::MSG,
    ) -> WinResult<()> {
        Err(Error::from(S_FALSE))
    }
}

impl IOleWindow_Impl for DitherPreviewHandler_Impl {
    fn GetWindow(&self) -> WinResult<HWND> {
        let g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        if !g.child.is_invalid() {
            Ok(g.child)
        } else if !g.parent.is_invalid() {
            Ok(g.parent)
        } else {
            record(Reason::InitFailed);
            Err(Error::from(E_FAIL))
        }
    }

    fn ContextSensitiveHelp(&self, _fentermode: BOOL) -> WinResult<()> {
        Ok(())
    }
}

impl IObjectWithSite_Impl for DitherPreviewHandler_Impl {
    fn SetSite(&self, punksite: windows::core::Ref<'_, IUnknown>) -> WinResult<()> {
        let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        g.site = punksite.ok().ok().cloned();
        Ok(())
    }

    fn GetSite(&self, riid: *const GUID, ppvsite: *mut *mut core::ffi::c_void) -> WinResult<()> {
        if riid.is_null() || ppvsite.is_null() {
            return Err(Error::from(E_POINTER));
        }
        unsafe {
            *ppvsite = std::ptr::null_mut();
        }
        let g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        let Some(site) = g.site.clone() else {
            return Err(Error::from(E_FAIL));
        };
        drop(g);
        unsafe { site.query(&*riid, ppvsite).ok() }
    }
}

impl IPreviewHandlerVisuals_Impl for DitherPreviewHandler_Impl {
    fn SetBackgroundColor(&self, color: COLORREF) -> WinResult<()> {
        let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        g.bg = color;
        let child = g.child;
        drop(g);
        repaint(child);
        Ok(())
    }

    fn SetFont(&self, plf: *const LOGFONTW) -> WinResult<()> {
        if plf.is_null() {
            return Err(Error::from(E_INVALIDARG));
        }
        let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        g.font = unsafe { *plf };
        g.has_font = true;
        Ok(())
    }

    fn SetTextColor(&self, color: COLORREF) -> WinResult<()> {
        let mut g = self.state.lock().map_err(|_| Error::from(E_FAIL))?;
        g.text = color;
        Ok(())
    }
}

fn do_preview(handler: &DitherPreviewHandler_Impl) -> WinResult<()> {
    let (stream, size, parent, rect) = {
        let g = handler.state.lock().map_err(|_| Error::from(E_FAIL))?;
        if g.parent.is_invalid() {
            record(Reason::InitFailed);
            return Err(Error::from(E_FAIL));
        }
        let stream = g.bound.stream.clone().ok_or_else(|| {
            record(Reason::InitFailed);
            Error::from(E_FAIL)
        })?;
        (stream, g.bound.size, g.parent, g.rect)
    };

    let side = (rect.right - rect.left)
        .unsigned_abs()
        .max((rect.bottom - rect.top).unsigned_abs())
        .clamp(1, 1024);
    let limits = ThumbLimits::default();
    let deadline = Instant::now() + Duration::from_millis(limits.deadline_ms);
    let mut io = StreamReadAt::new(stream, size, limits.max_read_at_ops, deadline);
    let extracted = extract_preview(&mut io, side, true, &limits);

    let mut g = handler.state.lock().map_err(|_| Error::from(E_FAIL))?;
    if !g.hbmp.is_invalid() {
        delete_bitmap(g.hbmp);
        g.hbmp = HBITMAP::default();
    }
    match extracted {
        Ok(bmp) => {
            let bgra = rgba_premul_to_bgra(&bmp.rgba);
            match create_bgra_premul_dib(bmp.width as i32, bmp.height as i32, &bgra) {
                Ok(hbmp) => {
                    g.hbmp = hbmp;
                    g.width = bmp.width;
                    g.height = bmp.height;
                    g.failed = false;
                }
                Err(_) => {
                    record(Reason::BitmapCreateFailed);
                    g.failed = true;
                    g.width = 0;
                    g.height = 0;
                }
            }
        }
        Err(reason) => {
            record(reason);
            g.failed = true;
            g.width = 0;
            g.height = 0;
        }
    }
    let state_ptr = &handler.state as *const Mutex<PreviewState>;
    if g.child.is_invalid() {
        drop(g);
        let child = create_child(parent, rect, state_ptr)?;
        let mut g = handler.state.lock().map_err(|_| Error::from(E_FAIL))?;
        g.child = child;
    } else {
        let child = g.child;
        drop(g);
        place_child(child, rect);
    }
    Ok(())
}

fn place_child(child: HWND, rect: RECT) {
    if child.is_invalid() {
        return;
    }
    let w = (rect.right - rect.left).max(0);
    let h = (rect.bottom - rect.top).max(0);
    unsafe {
        let _ = MoveWindow(child, rect.left, rect.top, w, h, true);
        let _ = InvalidateRect(Some(child), None, true);
    }
}

fn repaint(child: HWND) {
    if child.is_invalid() {
        return;
    }
    unsafe {
        let _ = InvalidateRect(Some(child), None, true);
    }
}

fn create_child(parent: HWND, rect: RECT, state: *const Mutex<PreviewState>) -> WinResult<HWND> {
    ensure_class()?;
    let class = class_name();
    let title: [u16; 1] = [0];
    let w = (rect.right - rect.left).max(0);
    let h = (rect.bottom - rect.top).max(0);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            rect.left,
            rect.top,
            w,
            h,
            Some(parent),
            None::<HMENU>,
            Some(module_instance()),
            Some(state as *const core::ffi::c_void),
        )
    }
}

fn class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| {
        "DitherPreviewPane"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    })
}

fn module_instance() -> windows::Win32::Foundation::HINSTANCE {
    let mut module = windows::Win32::Foundation::HMODULE::default();
    unsafe {
        let _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::PCWSTR(wnd_proc as *const () as *const u16),
            &mut module,
        );
    }
    windows::Win32::Foundation::HINSTANCE(module.0)
}

fn ensure_class() -> WinResult<()> {
    static READY: OnceLock<()> = OnceLock::new();
    if READY.get().is_some() {
        return Ok(());
    }
    let name = class_name();
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: module_instance(),
        hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
        hCursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR::default(),
        hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
        lpszMenuName: windows::core::PCWSTR::null(),
        lpszClassName: windows::core::PCWSTR(name.as_ptr()),
    };
    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        // Class may already exist in this process.
        let err = windows::core::Error::from_thread();
        if err.code() != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            return Err(err);
        }
    }
    let _ = READY.set(());
    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam.0 as *const CREATESTRUCTW;
        if !cs.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_PAINT {
        paint(hwnd);
        return LRESULT(0);
    }
    if msg == WM_DESTROY {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn paint(hwnd: HWND) {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Mutex<PreviewState>;
    if ptr.is_null() {
        return;
    }
    let Ok(g) = (unsafe { &*ptr }).lock() else {
        return;
    };
    let hbmp = g.hbmp;
    let bw = g.width;
    let bh = g.height;
    let bg = g.bg;
    let text = g.text;
    let failed = g.failed;
    let font = if g.has_font { Some(g.font) } else { None };
    drop(g);

    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        return;
    }
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let brush = unsafe { CreateSolidBrush(bg) };
    unsafe {
        FillRect(hdc, &client, brush);
        let _ = DeleteObject(brush.into());
    }
    if !failed && !hbmp.is_invalid() && bw > 0 && bh > 0 {
        blit_fit(hdc, hbmp, bw, bh, &client);
    } else {
        draw_fallback(hdc, &client, text, font);
    }
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

fn blit_fit(hdc: HDC, hbmp: HBITMAP, bw: u32, bh: u32, client: &RECT) {
    let fit = fit_centered(
        bw,
        bh,
        client.right - client.left,
        client.bottom - client.top,
    );
    if fit.w <= 0 || fit.h <= 0 {
        return;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        let old = SelectObject(mem, HGDIOBJ(hbmp.0));
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(
            hdc,
            client.left + fit.x,
            client.top + fit.y,
            fit.w,
            fit.h,
            mem,
            0,
            0,
            bw as i32,
            bh as i32,
            blend,
        );
        let _ = SelectObject(mem, old);
        let _ = DeleteDC(mem);
    }
}

fn draw_fallback(hdc: HDC, client: &RECT, text: COLORREF, font: Option<LOGFONTW>) {
    let mut label: Vec<u16> = "No preview"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let n = label.len() - 1;
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, text);
        let font_obj = font.map(|lf| CreateFontIndirectW(&lf));
        let old = font_obj.map(|f| SelectObject(hdc, f.into()));
        let mut rc = *client;
        let _ = DrawTextW(
            hdc,
            &mut label[..n],
            &mut rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        if let Some(old) = old {
            let _ = SelectObject(hdc, old);
        }
        if let Some(f) = font_obj {
            let _ = DeleteObject(f.into());
        }
    }
}

/// Used by the diag host so a destroyed top-level window ends its message loop.
pub fn post_quit() {
    unsafe { PostQuitMessage(0) };
}
