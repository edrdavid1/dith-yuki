//! IClassFactory for [`DitherThumbProvider`].

use crate::provider::{DitherThumbProvider, DLL_LOCK_COUNT};
use std::sync::atomic::Ordering;
use windows::core::{IUnknown, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_INVALIDARG};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::BOOL;
use windows_implement::implement;

#[implement(IClassFactory)]
pub struct DitherClassFactory;

impl DitherClassFactory {
    pub fn new() -> Self {
        DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for DitherClassFactory {
    fn drop(&mut self) {
        DLL_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for DitherClassFactory_Impl {
    fn CreateInstance(
        &self,
        punk_outer: windows::core::Ref<'_, IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> windows::core::Result<()> {
        if punk_outer.is_some() {
            return Err(windows::core::Error::from(CLASS_E_NOAGGREGATION));
        }
        if riid.is_null() || ppv.is_null() {
            return Err(windows::core::Error::from(E_INVALIDARG));
        }
        unsafe {
            *ppv = std::ptr::null_mut();
        }
        let provider: IUnknown = DitherThumbProvider::new().into();
        // SAFETY: riid/ppv validated; QueryInterface fills ppv on success.
        let hr = unsafe { provider.query(&*riid, ppv) };
        hr.ok()?;
        Ok(())
    }

    fn LockServer(&self, flock: BOOL) -> windows::core::Result<()> {
        if flock.as_bool() {
            DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            DLL_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// Exported COM entry points.
pub mod exports {
    use super::*;
    use crate::provider::DLL_LOCK_COUNT;
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_POINTER, S_FALSE, S_OK};
    use windows::Win32::System::Com::IClassFactory;

    fn parse_clsid() -> GUID {
        GUID::from_u128(0xBC7D0A00_220F_46DD_AAA8_C754864EE648)
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllGetClassObject(
        rclsid: *const GUID,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> HRESULT {
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        *ppv = std::ptr::null_mut();
        if *rclsid != parse_clsid() {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = DitherClassFactory::new().into();
        unsafe { factory.query(&*riid, ppv) }
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
        if DLL_LOCK_COUNT.load(Ordering::SeqCst) == 0 {
            S_OK
        } else {
            S_FALSE
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllRegisterServer() -> HRESULT {
        match crate::registry::register_user(None) {
            Ok(()) => S_OK,
            Err(_) => windows::Win32::Foundation::E_FAIL,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
        match crate::registry::unregister_user() {
            Ok(()) => S_OK,
            Err(_) => windows::Win32::Foundation::E_FAIL,
        }
    }
}
