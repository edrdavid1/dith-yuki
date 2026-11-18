import { useCallback, useContext } from 'react';
import {
  HapticFeedbackPattern,
  PerformanceTime,
  perform,
} from 'tauri-macos-haptics-api';
import { ShellContext } from '../app/shell/ShellContext';
import { performSliderHaptic } from '../shared/ipc';
import type { SliderHapticPattern } from './sliderHaptics';

function toPluginPattern(pattern: SliderHapticPattern): HapticFeedbackPattern {
  switch (pattern) {
    case 'alignment':
      return HapticFeedbackPattern.Alignment;
    case 'levelChange':
      return HapticFeedbackPattern.LevelChange;
    case 'generic':
      return HapticFeedbackPattern.Generic;
  }
}

/** Same remap as `tauri-macos-haptics-api` / `perform_slider_haptic`. */
function toInvokePattern(pattern: SliderHapticPattern): 0 | 1 | 2 {
  switch (pattern) {
    case 'alignment':
      return 0;
    case 'levelChange':
      return 1;
    case 'generic':
      return 2;
  }
}

/**
 * macOS Taptic Engine helper. No-ops when pref off.
 *
 * Always fires both:
 * 1. App command scheduled on the AppKit main thread
 * 2. Plugin `perform` from this webview
 *
 * Keep this path simple — popout eval routing masked failures and LevelChange
 * is silent for ordinary slider drags (it is for Force Click accelerators).
 */
export function useHapticFeedback() {
  const shell = useContext(ShellContext);
  const prefEnabled = shell?.hapticFeedback ?? true;

  const triggerHaptic = useCallback(
    (pattern: SliderHapticPattern = 'alignment'): void => {
      if (!prefEnabled) return;

      const invokePattern = toInvokePattern(pattern);
      void performSliderHaptic(invokePattern).catch((err) => {
        console.warn('[haptic] perform_slider_haptic failed', err);
      });
      void perform(toPluginPattern(pattern), PerformanceTime.Now).catch((err) => {
        console.warn('[haptic] plugin perform failed', err);
      });
    },
    [prefEnabled]
  );

  return { triggerHaptic, prefEnabled };
}
