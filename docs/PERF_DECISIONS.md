# Resize backdrop — decisions

Track A of the lite resize spec. No speed claim: this changes the color of the gap, not how fast the webview catches the window.

Checked against Tauri **2.11.5**, wry **0.55.1**, tao **0.35.3** (crate sources in the local cargo registry, 2026-09-28).

## A1. Window inventory

| Window | Created | Backdrop before this change | Backdrop after |
|---|---|---|---|
| Main (`main`) | `tauri.conf.json` (`create: false`) then `WebviewWindowBuilder::from_config` in `src-tauri/src/main.rs` | Config `backgroundColor` `#000000` (window and webview). macOS then overwrote the NSWindow with `#CDCDCD`. `index.html` painted `html, body, #root` `#000` | `#CDCDCD` on the config, the builder, `set_background_color` (both layers), macOS `underPageBackgroundColor`, and `html, body, #root` |
| Flex popout (`flex-popout-*`) | `on_new_window` in `main.rs`, opened by the patched `FloatingWindow` via `window.open` | No Tauri color. macOS NSWindow `#CDCDCD` after build. `popout.html` and the FloatingWindow patch already paint `#CDCDCD` | Same HTML. Builder `background_color` plus `set_background_color` and macOS under-page color at creation |
| Preferences, Help, New Project, bug report, Color Lab, pattern manager | React dialogs inside the main window (`AppLayout` and feature dialogs). Not `WebviewWindowBuilder` | Dialog CSS uses `--bg-window` (`#CDCDCD`). They share the main OS window | No separate OS window. Covered by the main window |
| Spike routes (`PopoutTestWindow`, `FlexLayoutPopoutTest`) | Frontend pages only. Not built as app windows | — | Not shipped windows |

No other `WebviewWindowBuilder` call sites.

## A2. APIs in Tauri 2.11.5

| API | What it sets | Format |
|---|---|---|
| `tauri.conf.json` `app.windows[].backgroundColor` | Window (tao `with_config`) and webview (`WebviewAttributes::from`) | `#RGB`, `#RRGGBB`, `#RRGGBBAA`, RGB array, or `{red,green,blue,alpha}` via `tauri_utils::config::Color` |
| `WebviewWindowBuilder::background_color` | Both layers (`webview_window.rs`) | `Color(r, g, b, a)`, 0–255 |
| `WebviewWindow::set_background_color` | Both layers | `Option<Color>` |
| `Window::set_background_color` / `Webview::set_background_color` | One layer each | same |

Platform notes from those crates:

- **Windows:** window alpha is ignored. WebView2 alpha is ignored when it is not `0` (Windows 8+). We pass `255`.
- **macOS window:** tao calls `NSWindow.setBackgroundColor`.
- **macOS webview:** `wry::wkwebview::set_background_color` is an empty function unless wry is built with the `transparent` feature. Tauri 2.11.5 does not expose that feature. Dev-mode resize still showed white bars after the window color, the HTML color, and `underPageBackgroundColor` were all `#CDCDCD`. That public color only paints overscroll past the page. The bars are WKWebView’s own view fill (`drawsBackground`, default white). The same private KVC key wry uses (`drawsBackground` = false) is set from `window_background.rs`, plus the view layer’s background color. The window stays opaque.

## A3. Themes

One theme. `--bg-window` and `--color-gray` are `#CDCDCD` in `frontend/src/shared/styles/tokens.css`. No light/dark switch, so popouts use the constant `window_background::THEME_BG` rather than reading a theme store.

The workspace column (`.app-layout`) is `--color-dark-gray` (`#999`). The native gap is the window chrome color (`#CDCDCD`), which matches the titlebar and docks. A collapsed dock can leave a `#CDCDCD` sliver against `#999`. That is the theme color the spec asks for, not white.

## A5. Not enabled

`transparent`, Mica, Acrylic, and Vibrancy are not set. The wry `transparent` cargo feature is not enabled.

## A6. Show after first frame

Main window: already `visible: false` in config and on the builder. `frontend/src/lib/boot.ts` calls `show()` after the first UI paint, or when the slow splash is ready. A 4s failsafe still calls `show()`.

Popouts stay visible at creation. The patched `FloatingWindow` (`frontend/src/patches/flexlayoutFloatingWindow.cjs`) reuses a named `window.open`, attaches on `load` / `focus` / `pageshow`, and close-polls `window.closed`. It never calls `show()`. Creating the popout hidden would leave a blank window. Popouts rely on the two color layers and the HTML background only.

## A7. `noRedirectionBitmap`

Not in the Tauri 2.11.5 API (no matches under the `tauri-2.11.5` crate). tao 0.35.3 has `WindowBuilderExtWindows::with_no_redirection_bitmap`, and it is not forwarded through Tauri. Not enabled. No before/after measurement, so there is no adoption.

## Layer check (dev mode, 2026-09-28)

Owner report: resize bars stayed white in `tauri dev` after the window, HTML, and `underPageBackgroundColor` were gray.

| Layer | Color at the time of the report | Could it be the white bar? |
|---|---|---|
| NSWindow | `#CDCDCD` via `set_background_color` | No. A window-colored gap would be gray |
| HTML `html, body, #root` | `#CDCDCD` in dev `index.html` | No. Unpainted native pixels sit under the page |
| `underPageBackgroundColor` | `#CDCDCD` | No. That API colors overscroll, and the bars stayed white |
| WKWebView `drawsBackground` | default white; Tauri/wry never sets it on macOS without the `transparent` feature | Yes. This is the view fill in the live-resize gap |

Follow-up: `drawsBackground` is set to false and the view layer background is `#CDCDCD`. The window stays opaque.

Owner check, dev mode, temporary blue page (`#1B4DFF`, window `#CDCDCD`): gray stripes appear while resizing. White stripes do not. The remaining gap is the NSWindow color showing where WKWebView has not caught the frame yet. That is the intended mitigation: the stripe matches the chrome (`#CDCDCD`) instead of staying white. It does not remove the asynchronous resize. The blue page was removed after that check and is not in the build.
