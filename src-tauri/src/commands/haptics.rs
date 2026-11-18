//! Slider / nudge haptics on the AppKit main thread.
//!
//! The plugin's `perform` is async (Tokio). Schedule the performer onto the
//! AppKit main run loop so pulses are not dropped.

use tauri::AppHandle;

/// Pattern ids mirror the plugin JS remap (0=Alignment, 1=LevelChange, 2=Generic).
#[tauri::command]
pub fn perform_slider_haptic(app: AppHandle, pattern: u64) -> Result<(), String> {
    // Must match channels that enable the `tauri-macos-haptics` optional dep.
    #[cfg(all(
        target_os = "macos",
        any(feature = "tauri-macos-haptics", feature = "channel-direct", feature = "channel-msstore")
    ))]
    {
        use tauri_macos_haptics::haptics::{
            HapticFeedbackManager, HapticPattern, PerformanceTime,
        };

        let pattern = match pattern {
            0 => HapticPattern::Alignment,
            1 => HapticPattern::LevelChange,
            2 => HapticPattern::Generic,
            other => {
                return Err(format!(
                    "Unknown haptic pattern {other} (expected 0=Alignment, 1=LevelChange, 2=Generic)."
                ));
            }
        };

        app.run_on_main_thread(move || {
            HapticFeedbackManager::default_performer()
                .perform(pattern, Some(PerformanceTime::Now));
        })
        .map_err(|e| format!("run_on_main_thread failed: {e}"))?;
        Ok(())
    }

    #[cfg(not(all(
        target_os = "macos",
        any(feature = "tauri-macos-haptics", feature = "channel-direct", feature = "channel-msstore")
    )))]
    {
        let _ = (app, pattern);
        Err("Haptic feedback is only supported on macOS builds with haptics enabled.".into())
    }
}
