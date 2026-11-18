import { invoke } from '@tauri-apps/api/core';

/** Pattern ids match `perform_slider_haptic` / the plugin remap. */
export type SliderHapticInvokePattern = 0 | 1 | 2;

/**
 * Fire a slider haptic on the AppKit main thread (sync Tauri command).
 * Prefer this over the plugin's async `perform` during live drag.
 */
export function performSliderHaptic(pattern: SliderHapticInvokePattern): Promise<void> {
  return invoke('perform_slider_haptic', { pattern });
}
