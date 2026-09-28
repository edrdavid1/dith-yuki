//! `IClassFactory` for the thumbnail provider and the preview handler.

use crate::preview::DitherPreviewHandler;
use crate::registry_keys::{CLSID_PREVIEW_U128, CLSID_THUMB_U128};
use crate::thumbnail::{DitherThumbProvider, DLL_LOCK_COUNT};
use std::sync::atomic::Ordering;
use windows::core::{IUnknown, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_INVALIDARG, E_POINTER, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::BOOL;
use windows_implement::implement;

#[derive(Clone, Copy)]
enum ShellClass {
    Thumb,
    Preview,
}

#[implement(IClassFactory)]
pub struct DitherClassFactory {
    class: ShellClass,
}

impl DitherClassFactory {
    fn new(class: ShellClass) -> Self {
        DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self { class }
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
        ppv: *mut *mut core::ffi::c_void,
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
        let unknown: IUnknown = match self.class {
            ShellClass::Thumb => DitherThumbProvider::new().into(),
            ShellClass::Preview => DitherPreviewHandler::new().into(),
        };
        // SAFETY: riid and ppv were checked. QueryInterface writes ppv.
        unsafe { unknown.query(&*riid, ppv).ok() }
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

pub mod exports {
    use super::*;
    use crate::guard::catch_com;

    fn clsid(value: u128) -> GUID {
        GUID::from_u128(value)
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllGetClassObject(
        rclsid: *const GUID,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        unsafe {
            *ppv = std::ptr::null_mut();
        }
        let class = unsafe {
            if *rclsid == clsid(CLSID_THUMB_U128) {
                ShellClass::Thumb
            } else if *rclsid == clsid(CLSID_PREVIEW_U128) {
                ShellClass::Preview
            } else {
                return CLASS_E_CLASSNOTAVAILABLE;
            }
        };
        let made = catch_com(|| {
            let factory: IClassFactory = DitherClassFactory::new(class).into();
            unsafe { factory.query(&*riid, ppv).ok() }
        });
        match made {
            Ok(()) => S_OK,
            Err(e) => e.code(),
        }
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
        match crate::registry::register(crate::registry::Hive::User, None) {
            Ok(()) => S_OK,
            Err(_) => windows::Win32::Foundation::E_FAIL,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
        match crate::registry::unregister(crate::registry::Hive::User) {
            Ok(()) => S_OK,
            Err(_) => windows::Win32::Foundation::E_FAIL,
        }
    }
}
