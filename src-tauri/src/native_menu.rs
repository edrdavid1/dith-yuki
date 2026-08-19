use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{App, AppHandle, Emitter, Runtime};

const EVENT: &str = "native-menu";

pub fn install(app: &App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        suppress_system_edit_injections();
        match build(app).and_then(|menu| app.set_menu(menu).map(|_| ())) {
            Ok(()) => strip_system_edit_items(),
            Err(err) => log::error!("native macOS menu failed: {err}"),
        }
        observe_edit_menu_for_system_items();
    }
    Ok(())
}

pub fn emit_event<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    let id = event.id().0.as_str();
    if id.is_empty() {
        return;
    }
    let _ = app.emit(EVENT, id);
}

#[cfg(target_os = "macos")]
fn build(app: &App) -> tauri::Result<Menu<tauri::Wry>> {
    let about = MenuItem::with_id(app, "about", "About Dither Yuki", true, None::<&str>)?;
    let preferences = MenuItem::with_id(
        app,
        "preferences",
        "Preferences…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let check_updates =
        MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?;

    let app_menu = Submenu::with_items(
        app,
        "Dither Yuki",
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &preferences,
            &check_updates,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Dither Yuki", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;

    let new_project = MenuItem::with_id(
        app,
        "new-project",
        "New Project…",
        true,
        Some("CmdOrCtrl+N"),
    )?;
    let open_image =
        MenuItem::with_id(app, "open-image", "Open Image", true, Some("CmdOrCtrl+O"))?;
    let import_layer = MenuItem::with_id(
        app,
        "import-image-layer",
        "Import Image as Layer…",
        true,
        None::<&str>,
    )?;
    let open_project = MenuItem::with_id(
        app,
        "open-project",
        "Open Project…",
        true,
        Some("CmdOrCtrl+Shift+O"),
    )?;
    let save_project =
        MenuItem::with_id(app, "save-project", "Save Project", true, Some("CmdOrCtrl+S"))?;
    let save_project_as = MenuItem::with_id(
        app,
        "save-project-as",
        "Save Project As…",
        true,
        Some("CmdOrCtrl+Shift+S"),
    )?;
    let save_export = MenuItem::with_id(app, "save-export", "Save/Export", true, None::<&str>)?;

    let file_menu = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &new_project,
            &open_image,
            &import_layer,
            &open_project,
            &PredefinedMenuItem::separator(app)?,
            &save_project,
            &save_project_as,
            &save_export,
        ],
    )?;

    let undo = MenuItem::with_id(app, "undo", "Undo", true, Some("CmdOrCtrl+Z"))?;
    let redo = MenuItem::with_id(app, "redo", "Redo", true, Some("CmdOrCtrl+Shift+Z"))?;
    let edit_menu = Submenu::with_items(app, "Edit", true, &[&undo, &redo])?;

    let export_pattern =
        MenuItem::with_id(app, "export-pattern", "Export Pattern…", true, None::<&str>)?;
    let import_pattern =
        MenuItem::with_id(app, "import-pattern", "Import Pattern…", true, None::<&str>)?;
    let presets_menu =
        Submenu::with_items(app, "Presets", true, &[&export_pattern, &import_pattern])?;

    let color_lab = MenuItem::with_id(app, "color-lab", "Open Color Lab", true, None::<&str>)?;
    let color_lab_menu = Submenu::with_items(app, "Color Lab", true, &[&color_lab])?;

    let help_item = MenuItem::with_id(app, "help", "Dither Yuki Help", true, None::<&str>)?;
    let help_updates =
        MenuItem::with_id(app, "help-check-updates", "Check for Updates…", true, None::<&str>)?;
    let help_menu = Submenu::with_items(app, "Help", true, &[&help_item, &help_updates])?;

    Menu::with_items(
        app,
        &[
            &app_menu,
            &file_menu,
            &edit_menu,
            &presets_menu,
            &color_lab_menu,
            &help_menu,
        ],
    )
}

/// AppKit appends Writing Tools / AutoFill / Dictation / Emoji to any menu titled "Edit".
#[cfg(target_os = "macos")]
fn suppress_system_edit_injections() {
    use cocoa::base::{id, nil, YES};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let defaults: id = msg_send![class!(NSUserDefaults), standardUserDefaults];
        for key in [
            "NSDisabledDictationMenuItem",
            "NSDisabledCharacterPaletteMenuItem",
        ] {
            let ns_key: id = NSString::alloc(nil).init_str(key);
            let _: () = msg_send![defaults, setBool: YES forKey: ns_key];
        }
    }
}

#[cfg(target_os = "macos")]
fn strip_system_edit_items() {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let main_menu: id = msg_send![app, mainMenu];
        if main_menu == nil {
            return;
        }
        let edit_title: id = NSString::alloc(nil).init_str("Edit");
        let edit_item: id = msg_send![main_menu, itemWithTitle: edit_title];
        if edit_item == nil {
            return;
        }
        let edit_menu: id = msg_send![edit_item, submenu];
        if edit_menu == nil {
            return;
        }
        strip_unwanted_edit_items(edit_menu);
    }
}

#[cfg(target_os = "macos")]
unsafe fn strip_unwanted_edit_items(edit_menu: cocoa::base::id) {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{msg_send, sel, sel_impl};

    let items: id = msg_send![edit_menu, itemArray];
    let copy: id = msg_send![items, copy];
    if copy == nil {
        return;
    }
    let count: usize = msg_send![copy, count];
    for i in 0..count {
        let item: id = msg_send![copy, objectAtIndex: i];
        if item == nil {
            continue;
        }
        let is_sep: bool = msg_send![item, isSeparatorItem];
        if is_sep {
            let _: () = msg_send![edit_menu, removeItem: item];
            continue;
        }
        let title: id = msg_send![item, title];
        let undo: id = NSString::alloc(nil).init_str("Undo");
        let redo: id = NSString::alloc(nil).init_str("Redo");
        let is_undo: bool = msg_send![title, isEqualToString: undo];
        let is_redo: bool = msg_send![title, isEqualToString: redo];
        if !is_undo && !is_redo {
            let _: () = msg_send![edit_menu, removeItem: item];
        }
    }
}

#[cfg(target_os = "macos")]
fn observe_edit_menu_for_system_items() {
    use block::ConcreteBlock;
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let name: id = NSString::alloc(nil).init_str("NSMenuDidBeginTrackingNotification");
        let block = ConcreteBlock::new(|notification: id| {
            unsafe {
                let menu: id = msg_send![notification, object];
                if menu == nil {
                    return;
                }
                let count: usize = msg_send![menu, numberOfItems];
                if count == 0 {
                    return;
                }
                let first: id = msg_send![menu, itemAtIndex: 0usize];
                if first == nil {
                    return;
                }
                let title: id = msg_send![first, title];
                let undo: id = NSString::alloc(nil).init_str("Undo");
                let is_undo: bool = msg_send![title, isEqualToString: undo];
                if is_undo {
                    strip_unwanted_edit_items(menu);
                }
            }
        });
        let block = block.copy();
        let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
        let _: id = msg_send![
            center,
            addObserverForName: name
            object: nil
            queue: nil
            usingBlock: &*block
        ];
        std::mem::forget(block);
    }
}
