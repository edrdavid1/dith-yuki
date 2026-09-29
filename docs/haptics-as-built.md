# macOS slider haptics — as built

Tactile feedback on sliders and number-input arrow nudges via the macOS
Taptic Engine (`NSHapticFeedbackManager`). Ships from **1.0.5-beta**.

## Stack

| Layer | Piece |
|---|---|
| Rust plugin | `tauri-macos-haptics` v4 (workspace dep of `src-tauri`) |
| Init | `tauri_macos_haptics::init()` in `src-tauri/src/main.rs` |
| Capability | `"tauri-macos-haptics:default"` in `capabilities/default.json` |
| JS API | `tauri-macos-haptics-api` (`isSupported`, `perform`) |
| Hook | `frontend/src/hooks/useHapticFeedback.ts` |
| Drag policy | `frontend/src/hooks/sliderHaptics.ts` (`evaluateSliderHaptic`) |
| UI | `Slider.tsx`, `NumberInput.tsx` |
| Pref | Preferences → **Tactile feedback on sliders** (`ShellContext.hapticFeedback`, default on) |

Off-macOS / unsupported hardware: `isSupported()` is false and calls no-op.
Plugin builds on all targets.

## Trigger rules

- **Drag stream:** `LevelChange` when the snapped value changes, throttled to
  `MIN_INTERVAL_MS` (40 ms ≈ 25 Hz).
- **Min / max:** `Generic` when newly hitting a boundary (ignores throttle).
- **Arrow nudge:** `Alignment` from `NumberInput` / slider nudge paths.
- Fire-and-forget (`PerformanceTime.Now`); never await inside input handlers.
- Honors the Preferences toggle; no backend custom command — uses the plugin API.

## Limits

- Requires Force Touch / Taptic hardware and finger contact on the trackpad
  (system limitation).
- Mouse / non-macOS: silent no-op; sliders still work.
- User can disable system haptic feedback in Accessibility settings.
