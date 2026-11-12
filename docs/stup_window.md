Here is the English translation of the document.

---

**Instruction: Splash window at startup (Tauri 2, macOS)**  
12 Dec 2026 · @Dave

The splash window is shown immediately at startup, the main window stays hidden and appears on the `boot_finished` signal. The user sees the app reacting instead of another Dock bounce.

## 1. How it works
1. Tauri creates two windows from the config: `splash` (visible) and `main` (hidden).
2. Splash shows the logo and stage text while the engine, layout, and React are loading.
3. The frontend calls `boot_finished` after the first frame. Rust waits for a minimum display time (600 ms), shows `main`, and destroys `splash`.
4. If loading hangs, a watchdog timer forces `main` to show after 15 seconds.

The system icon bounce at launch remains as is; we don't touch or imitate it. The manual bounce at startup must be disabled (section 6).

## 2. Window configuration
In `src-tauri/tauri.conf.json`, under `app.windows`. Keep your existing `main` settings, just add `"visible": false`.

```json
{
  "app": {
    "windows": [
      {
        "label": "splash",
        "url": "splash.html",
        "title": "Dither Yuki",
        "width": 480,
        "height": 300,
        "center": true,
        "decorations": false,
        "resizable": false,
        "closable": false,
        "minimizable": false,
        "maximizable": false,
        "skipTaskbar": true,
        "shadow": true,
        "visible": true,
        "focus": true
      },
      {
        "label": "main",
        "url": "index.html",
        "visible": false
      }
    ]
  }
}
```

What matters here:
- **No `transparent`.** A transparent window on macOS requires `macOSPrivateApi`, which is incompatible with Mac App Store publishing. So the splash is rectangular, with a system shadow (`shadow: true`).
- **No `alwaysOnTop`.** Otherwise the splash stays above other apps if the user switches during loading.
- **`closable: false`.** The user won't accidentally close the splash. Rust closes it via `destroy()`, independent of this flag.
- If your Tauri version supports `backgroundColor` for windows, set a color matching the splash background. This removes the white flash before the page loads.

## 3. `splash.html` file
Place it in `frontend/public/splash.html`. Everything in `public` is copied to the build root as is, and in dev it's served at `/splash.html`. This is the same technique as your `popout.html`.

The page is intentionally React-free and build-free: static HTML and CSS so it appears as early as possible. Insert the logo as inline SVG; no external files needed.

```html
<!doctype html>
<html lang='en'>
<head>
<meta charset='utf-8'>
<meta name='viewport' content='width=device-width, initial-scale=1'>
<title>Dither Yuki</title>
<style>
  :root { --bg:#f4f4f2; --fg:#1b1b1b; --muted:#6b6b6b; --accent:#1b1b1b; --track:#dcdcd8; }
  @media (prefers-color-scheme: dark) {
    :root { --bg:#1b1b1b; --fg:#f2f2f0; --muted:#9a9a96; --accent:#f2f2f0; --track:#333330; }
  }
  html, body {
    margin:0; height:100%; background:var(--bg); color:var(--fg);
    font:13px -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    -webkit-user-select:none; user-select:none; cursor:default; overflow:hidden;
  }
  main { height:100%; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:14px; }
  svg { width:72px; height:72px; }
  h1 { margin:0; font-size:20px; font-weight:600; letter-spacing:.02em; }
  #stage { color:var(--muted); min-height:1.2em; }
  .bar { width:180px; height:3px; background:var(--track); border-radius:2px; overflow:hidden; }
  .bar i { display:block; width:40%; height:100%; background:var(--accent); border-radius:2px; animation:slide 1.2s ease-in-out infinite; }
  @keyframes slide { from { transform:translateX(-100%); } to { transform:translateX(250%); } }
  @media (prefers-reduced-motion: reduce) { .bar i { animation:none; width:100%; opacity:.4; } }
</style>
</head>
<body>
<main>
  <!-- replace with the real logo -->
  <svg viewBox='0 0 72 72' aria-hidden='true'><rect width='72' height='72' rx='16' fill='currentColor'/></svg>
  <h1>Dither Yuki</h1>
  <div id='stage'>Starting…</div>
  <div class='bar'><i></i></div>
</main>
<script>
  window.setStage = function (text) {
    document.getElementById('stage').textContent = text;
  };
</script>
</body>
</html>
```

The stage text is updated by calling `window.setStage(...)`, which Rust executes via `eval`. So the splash needs no IPC, no capability, no `withGlobalTauri`.

## 4. Rust: `splash.rs` module
Create `src-tauri/src/splash.rs`.

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Minimum display time so the splash doesn't flash on fast startup.
const MIN_SPLASH: Duration = Duration::from_millis(600);
/// If the frontend doesn't respond within this time, show main forcibly.
const BOOT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct SplashState {
    started: Instant,
    revealed: AtomicBool,
}

impl SplashState {
    pub fn new() -> Self {
        Self { started: Instant::now(), revealed: AtomicBool::new(false) }
    }
}

/// Idempotent: repeated calls do nothing.
pub fn reveal_main(app: &AppHandle) {
    let state = app.state::<SplashState>();
    if state.revealed.swap(true, Ordering::SeqCst) {
        return;
    }
    // Show main first, then close splash: no windowless gap.
    if let Some(main) = app.get_webview_window("main") {
        if let Err(e) = main.show() { eprintln!("[splash] main.show failed: {e}"); }
        if let Err(e) = main.set_focus() { eprintln!("[splash] main.set_focus failed: {e}"); }
    } else {
        eprintln!("[splash] main window not found");
    }
    if let Some(splash) = app.get_webview_window("splash") {
        if let Err(e) = splash.destroy() { eprintln!("[splash] splash.destroy failed: {e}"); }
    }
}

pub fn set_stage(app: &AppHandle, text: &str) {
    if let Some(splash) = app.get_webview_window("splash") {
        let arg = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
        let _ = splash.eval(&format!("window.setStage && window.setStage({arg})"));
    }
}

pub fn start_watchdog(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(BOOT_TIMEOUT);
        if !app.state::<SplashState>().revealed.load(Ordering::SeqCst) {
            eprintln!("[splash] boot timeout, forcing reveal");
            reveal_main(&app);
        }
    });
}

#[tauri::command]
pub fn boot_finished(app: AppHandle) {
    let started = app.state::<SplashState>().started;
    let remaining = MIN_SPLASH.saturating_sub(started.elapsed());
    // Separate thread: don't block the command handler and avoid window deadlock.
    std::thread::spawn(move || {
        std::thread::sleep(remaining);
        reveal_main(&app);
    });
}

#[tauri::command]
pub fn boot_stage(app: AppHandle, text: String) {
    set_stage(&app, &text);
}
```

Wiring in `main.rs`. `generate_handler!` is called once, so add the two commands to your existing list, don't create a second one.

```rust
mod splash;

tauri::Builder::default()
    .manage(splash::SplashState::new())
    .setup(|app| {
        splash::start_watchdog(app.handle());
        splash::set_stage(app.handle(), "Starting…");
        // your existing initialization
        Ok(())
    })
    .invoke_handler(tauri::generate_handler![
        splash::boot_finished,
        splash::boot_stage,
        // your existing commands
    ])
```

`serde_json` is already in Tauri's dependencies. If it's not explicitly declared in your `Cargo.toml`, add it.

## 5. Frontend: `boot.ts`
Replaces the current window-showing logic in `finishBoot()`. Remove the direct `show()` call from JS; showing is now handled only by Rust.

```ts
import { invoke } from '@tauri-apps/api/core';

let done = false;

export async function finishBoot(): Promise<void> {
  if (done) return;
  done = true;
  // Two frames: the first UI frame is already painted, the window won't show a blank page.
  await new Promise<void>((r) =>
    requestAnimationFrame(() => requestAnimationFrame(() => r())),
  );
  try {
    await invoke('boot_finished');
  } catch (e) {
    console.error('[boot] boot_finished failed', e);
  }
}

export function bootStage(text: string): void {
  invoke('boot_stage', { text }).catch((e) => console.debug('[boot] boot_stage failed', e));
}
```

When to call `finishBoot()`: only when everything the user will see immediately is ready. For your app, that order is:
1. clean up orphaned `flex-popout-*` windows (if you implement W5 from the spec);
2. load `layout.json`, normalize, and `dockAllFloating`;
3. initialize the engine and shaders needed for the first screen;
4. first React render, then `finishBoot()`.

Between steps you can call `bootStage('Restoring layout…')`, `bootStage('Starting engine…')` so the user sees progress.

## 6. Dock bounce from `macos_dock_bounce.rs`
- Remove the call at startup. The system bounce and your second bounce overlap, and it looked bad.
- Keep it for post-startup events (export ready, user response needed): Informational only, once per event.
- Reset the request on focus return: `window.request_user_attention(None)`.
- macOS only bounces if the app is not currently active. If the window is focused, the call does nothing.

## 7. Verification
Test only on a built `.app` launched from Dock or Finder. In `tauri dev` the app starts from the terminal, and icon behavior is different.
1. Cold start: splash appears immediately, main window appears later without a white flash.
2. Fast start (app already cached): splash is visible for at least 600 ms, doesn't flicker.
3. Artificially slow down loading (e.g., `sleep` in engine init): splash stays, stages update, after 15 seconds main window is forced to show.
4. Don't call `finishBoot()` at all: watchdog fires and console shows `[splash] boot timeout`.
5. Cmd+Q during splash quits the app without stuck processes.
6. Light and dark theme: splash matches the system.
7. "Reduce motion" in system: progress bar doesn't animate.
8. Two monitors: splash appears on the expected screen, main window restores its position.
9. Relaunching an already open app doesn't create a second splash (if you have single-instance enabled).

## 8. Common issues

| Symptom | Cause and solution |
|---------|-------------------|
| Splash appears with a delay of tenths of a second | This is the cold start of the WebView itself, can't be avoided with settings. If you need an instant splash, you'll have to make a native window in Rust/AppKit, that's a separate task. |
| White flash on open | Set the background color in `splash.html` (already there) and `backgroundColor` in the window config. |
| App quits when splash closes | Check that `main` exists in the config (a hidden window counts as open). Use `destroy()` for closing, as in `reveal_main`. |
| Main window doesn't show | `boot_finished` isn't called (check console and init order) or the command isn't added to `generate_handler!`. |
| Double Dock bounce remains | Manual bounce is still called at startup (section 6). |
| Splash not found at `splash.html` | The file must be in `public/`, not `src/`, so it lands in the build root without processing. |

## 9. Out of scope for this instruction
- Native AppKit splash that appears before the WebView.
- Identical behavior on Windows and Linux: the config works there too, but the Dock verification applies only to macOS.