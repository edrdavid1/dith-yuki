//! Make every webview accept the first mouse click that activates its window.
//!
//! macOS default: `-[NSView acceptsFirstMouse:]` returns NO. Tauri's
//! `WryWebView` subclasses `WKWebView` and implements the method itself,
//! returning the `accept_first_mouse` ivar (default NO). Patching `WKWebView`
//! does not run for that subclass, so the first click on an inactive
//! `flex-popout-*` (Layers or Effect, once the other has key focus) is
//! consumed as window activation. No `mousedown`, no `pointerdown`, no `click`
//! reaches the chooser, and adding a layer takes two clicks.
//!
//! Windows are built with `WebviewWindowBuilder::accept_first_mouse(true)`
//! (`webview_debug::apply`). This class override covers a webview created
//! without that flag: `WryWebView` when it is already registered, and
//! `WKWebView` for anything that still inherits the stock method.

#![allow(deprecated)]
#![allow(unexpected_cfgs)]
#![allow(clippy::manual_c_str_literals)]

use std::os::raw::c_char;
use std::sync::Once;

use objc::runtime::{class_addMethod, Class, Imp, Object, Sel, BOOL, YES};
use objc::{sel, sel_impl};

// objc 0.2.7 exposes `class_addMethod` but not `class_replaceMethod` — declare
// the fallback ourselves against libobjc so a future WebKit that ships its own
// `acceptsFirstMouse:` override still gets patched.
#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn class_replaceMethod(cls: *mut Class, name: Sel, imp: Imp, types: *const c_char) -> Imp;
}

/// `-[WKWebView acceptsFirstMouse:]` → always YES.
extern "C" fn accepts_first_mouse(_this: &Object, _sel: Sel, _event: *mut Object) -> BOOL {
    YES
}

/// Install the class-level override. Idempotent; the work runs once.
pub fn install_accepts_first_mouse_override() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // `WryWebView` is registered when wry builds the first webview, which
        // is after this setup hook. Patch it if it is already loaded; the
        // builder flag covers the windows we create afterwards.
        for name in ["WryWebView", "WKWebView"] {
            let Some(cls) = Class::get(name) else {
                continue;
            };
            patch_accepts_first_mouse(cls as *const Class as *mut Class);
        }
    });
}

unsafe fn patch_accepts_first_mouse(cls: *mut Class) {
    let sel = sel!(acceptsFirstMouse:);
    // Objective-C type encoding: BOOL(id, SEL, id) → `c@:@`. (`c` is BOOL,
    // which on non-aarch64 is `signed char`; on aarch64 the runtime still
    // accepts `c` here — the objc crate maps BOOL to `bool` internally.)
    let types: *const c_char = b"c@:@\0".as_ptr() as *const c_char;
    let imp: Imp = std::mem::transmute::<extern "C" fn(&Object, Sel, *mut Object) -> BOOL, Imp>(
        accepts_first_mouse,
    );

    // Stock WKWebView inherits the method, so `class_addMethod` succeeds.
    // `WryWebView` already implements it, so add fails and we replace.
    let added: BOOL = class_addMethod(cls, sel, imp, types);
    // On aarch64 BOOL = bool (YES = true, NO = false); on x86 it is c_schar
    // (YES = 1, NO = 0). Compare against the runtime's NO to avoid the
    // aarch64-vs-x86 integer/bool mismatch.
    if added == objc::runtime::NO {
        let _prev = class_replaceMethod(cls, sel, imp, types);
    }
}
