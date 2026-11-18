# macOS slider haptics — as built

Tactile feedback on sliders and number-input arrow nudges via the macOS
Taptic Engine (`NSHapticFeedbackManager`). Ships from **1.0.5-beta**.

## Stack

| Layer | Piece |
|---|---|
| Rust plugin | `tauri-macos-haptics` v4 (optional dep of `src-tauri`) |
| Init | `tauri_macos_haptics::init()` in `src-tauri/src/main.rs` when the feature is on |
| App command | `perform_slider_haptic` — schedules the performer on the AppKit main thread |
| Capability | `"tauri-macos-haptics:default"` via channel config (`channels/direct.conf.json`, `channels/msstore.conf.json`) |
| JS API | `tauri-macos-haptics-api` (`perform`) + `shared/ipc/haptics.ts` |
| Hook | `frontend/src/hooks/useHapticFeedback.ts` |
| Drag policy | `frontend/src/hooks/sliderHaptics.ts` (`evaluateSliderHaptic`) |
| UI | `Slider.tsx`, `NumberInput.tsx` |
| Pref | Preferences → **Tactile feedback on sliders** (`ShellContext.hapticFeedback`, default on) |

Channels: **direct** and **msstore** enable haptics. **MAS** omits the plugin (private AppKit API).

Off-macOS / unsupported hardware: calls no-op. Feedback is only felt with a Force Touch trackpad and finger contact.

## Trigger rules

- **Drag stream:** `Alignment` when the snapped value changes, throttled to
  `MIN_INTERVAL_MS` (40 ms ≈ 25 Hz).
- **Min / max:** `Generic` when newly hitting a boundary (ignores throttle).
- **Arrow nudge:** `Alignment` from `NumberInput` / slider nudge paths.
- Fire-and-forget (`PerformanceTime.Now`); never await inside input handlers.
- Honors the Preferences toggle.

Do **not** use `LevelChange` for HTML slider ticks — AppKit reserves that pattern
for Force Click multilevel accelerators; it is silent during a normal drag.

## Flex-popout / docked panels

Slider drag attaches pointer listeners to the track (pointer capture), so moves
work in both the main window and a floated `flex-popout-*` document. Haptics use
the same React hook in both cases (one JS tree; portal into the popout).

## Bug fix (2026-11): plugin never registered

**Symptom:** Slider tactile feedback silent on direct builds even though the
crate was linked and Preferences showed the toggle.

**Cause:** `channel-direct` / `channel-msstore` listed
`dep:tauri-macos-haptics`. The `dep:` form enables the optional dependency but
does **not** set `cfg(feature = "tauri-macos-haptics")`. Plugin init and the
main-thread command were compiled out / always erred; frontend `perform` also
failed because the plugin was never registered.

**Fix:** List the optional-dep **feature name** in Cargo features:

```toml
channel-direct = [
    # …
    "tauri-macos-haptics",  # not "dep:tauri-macos-haptics"
]
channel-msstore = ["tauri-macos-haptics"]
```

Verify with a rebuild that rustc receives
`--cfg 'feature="tauri-macos-haptics"'`.

Related hardening in the same pass:

- Slider drag uses the track’s browsing context (pointer capture) so popout
  windows receive move/up (avoids “press only” + haptic burst on redock from
  stuck main-document listeners).
- Drag stream pattern is `Alignment`, not `LevelChange`.
- `accept_first_mouse(true)` on all webviews so the first click in an inactive
  popout reaches the chooser / controls.

## Limits

- Requires Force Touch / Taptic hardware and finger contact on the trackpad
  (system limitation).
- Mouse / non-macOS: silent no-op; sliders still work.
- User can disable system haptic feedback in Accessibility / Trackpad settings.
