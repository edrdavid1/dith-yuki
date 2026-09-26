//! Make WKWebView accept the first mouse click that activates its window.
//!
//! macOS default: `-[NSView acceptsFirstMouse:]` returns NO, and WKWebView
//! inherits that. When one of our windows is inactive (e.g. Preview is floated
//! into a `flex-popout-*` OS window whose `set_focus()` made it the key window,
//! leaving the main window inactive), the first click on the inactive window is
//! consumed by AppKit **for window activation** and never delivered to the
//! WKWebView. No `mousedown`, no `pointerdown`, no `click` fires in JS — so the
//! React `onPointerDown` handlers on the Layers "+" button and the
//! EffectSettingsPanel / EffectChooserDialog tiles never see the first click,
//! and adding a layer/effect requires two clicks.
//!
//! Overriding `-[WKWebView acceptsFirstMouse:]` to return YES makes AppKit
//! deliver the activation click to the webview, so a single click both focuses
//! the window and triggers the React handler.
//!
//! The override is applied once, at process level, against the WKWebView class.
//! This is safe: every webview in the app is user UI that should react to a
//! click regardless of which of our windows currently has key focus.

#![allow(deprecated)]
#![allow(unexpected_cfgs)]
#![allow(clippy::manual_c_str_literals)]

use std::os::raw::c_char;
use std::sync::Once;

use objc::runtime::{class_addMethod, Class, Imp, Object, Sel, BOOL, YES};
use objc::{class, sel, sel_impl};

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

/// Install the class-level override on WKWebView. Idempotent (call from any
/// window setup path); the actual work runs once.
pub fn install_accepts_first_mouse_override() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let wk_class = class!(WKWebView) as *const Class as *mut Class;
        let sel = sel!(acceptsFirstMouse:);
        // Objective-C type encoding: BOOL(id, SEL, id) → `c@:@`. (`c` is BOOL,
        // which on non-aarch64 is `signed char`; on aarch64 the runtime still
        // accepts `c` here — the objc crate maps BOOL to `bool` internally.)
        let types: *const c_char = b"c@:@\0".as_ptr() as *const c_char;
        let imp: Imp = std::mem::transmute::<extern "C" fn(&Object, Sel, *mut Object) -> BOOL, Imp>(
            accepts_first_mouse,
        );

        // WKWebView normally inherits `acceptsFirstMouse:` from NSView, so
        // `class_addMethod` succeeds and adds the override. If a future WebKit
        // ever ships its own override, `class_addMethod` returns NO and we fall
        // back to `class_replaceMethod` to still take effect.
        let added: BOOL = class_addMethod(wk_class, sel, imp, types);
        // On aarch64 BOOL = bool (YES = true, NO = false); on x86 it is c_schar
        // (YES = 1, NO = 0). Compare against the runtime's NO to avoid the
        // aarch64-vs-x86 integer/bool mismatch.
        if added == objc::runtime::NO {
            let _prev = class_replaceMethod(wk_class, sel, imp, types);
        }
    });
}
