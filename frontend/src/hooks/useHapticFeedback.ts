import { useCallback, useContext, useRef } from 'react';
import {
  HapticFeedbackPattern,
  PerformanceTime,
  isSupported,
  perform,
} from 'tauri-macos-haptics-api';
import { ShellContext } from '../app/shell/ShellContext';
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

/**
 * macOS Taptic Engine helper. No-ops when unsupported, pref off, or off-macOS.
 * Always fire-and-forget with PerformanceTime.Now — never await in callers.
 */
export function useHapticFeedback() {
  const shell = useContext(ShellContext);
  const prefEnabled = shell?.hapticFeedback ?? true;
  const supportedRef = useRef<boolean | null>(null);
  const supportInflightRef = useRef<Promise<boolean> | null>(null);

  const checkSupport = useCallback((): Promise<boolean> => {
    if (supportedRef.current !== null) return Promise.resolve(supportedRef.current);
    if (!supportInflightRef.current) {
      supportInflightRef.current = isSupported()
        .then((ok) => {
          supportedRef.current = ok;
          return ok;
        })
        .catch(() => {
          supportedRef.current = false;
          return false;
        })
        .finally(() => {
          supportInflightRef.current = null;
        });
    }
    return supportInflightRef.current;
  }, []);

  const triggerHaptic = useCallback(
    (pattern: SliderHapticPattern = 'levelChange'): void => {
      if (!prefEnabled) return;

      void checkSupport().then((ok) => {
        if (!ok) return;
        // Fire-and-forget — do not await; swallow errors.
        void perform(toPluginPattern(pattern), PerformanceTime.Now).catch(() => {});
      });
    },
    [checkSupport, prefEnabled]
  );

  return { triggerHaptic, checkSupport, prefEnabled };
}
