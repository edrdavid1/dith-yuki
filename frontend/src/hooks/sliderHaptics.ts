/**
 * Slider drag haptics — value-change + hard throttle only.
 *
 * Trigger when `newValue !== lastHapticValue` (every distinct snapped unit).
 * The only overload guard is MIN_INTERVAL_MS via performance.now().
 * Alignment for the stream (discrete ticks while dragging); Generic for
 * min/max. Do not use LevelChange here — AppKit reserves that for Force Click
 * multilevel accelerators, and it is silent during a normal HTML slider drag.
 */

export type SliderHapticPattern = 'alignment' | 'levelChange' | 'generic';

/**
 * Hard ceiling between LevelChange pulses (ms). Tune on a Force Touch trackpad:
 * lower → denser fast-drag buzz; higher → sparser. 40 ms ≈ 25 Hz safety margin.
 */
export const MIN_INTERVAL_MS = 40;

/** @deprecated Alias — prefer {@link MIN_INTERVAL_MS}. */
export const HAPTIC_MIN_INTERVAL_MS = MIN_INTERVAL_MS;

export type HapticMotionState = {
  lastHapticValue: number;
  lastHapticTime: number;
};

export type SliderHapticDecision = {
  trigger: boolean;
  pattern?: SliderHapticPattern;
  state: HapticMotionState;
};

export function createHapticMotionState(value: number): HapticMotionState {
  return {
    lastHapticValue: value,
    lastHapticTime: 0,
  };
}

/**
 * Decide whether a drag sample should pulse.
 * Boundaries (min/max) always fire Generic when newly hit, ignoring throttle.
 * Stream pulses Alignment when the value changed and ≥ MIN_INTERVAL_MS elapsed.
 */
export function evaluateSliderHaptic(args: {
  prev: number;
  next: number;
  min: number;
  max: number;
  now: number;
  state: HapticMotionState;
  minIntervalMs?: number;
}): SliderHapticDecision {
  const {
    prev,
    next,
    min,
    max,
    now,
    state,
    minIntervalMs = MIN_INTERVAL_MS,
  } = args;

  if (!Number.isFinite(next) || next === prev) {
    return { trigger: false, state };
  }

  const atMin = next <= min;
  const atMax = next >= max;
  const wasAtMin = prev <= min;
  const wasAtMax = prev >= max;
  const hitBoundary = (atMin && !wasAtMin) || (atMax && !wasAtMax);

  if (hitBoundary) {
    // Generic regardless of throttle — end-stops must always click.
    return {
      trigger: true,
      pattern: 'generic',
      state: { lastHapticValue: next, lastHapticTime: now },
    };
  }

  if (next === state.lastHapticValue) {
    return { trigger: false, state };
  }

  const elapsed = state.lastHapticTime > 0 ? now - state.lastHapticTime : Infinity;
  if (elapsed < minIntervalMs) {
    // Throttled: do not advance lastHapticValue so the unit still pending can fire later.
    return { trigger: false, state };
  }

  return {
    trigger: true,
    pattern: 'alignment',
    state: { lastHapticValue: next, lastHapticTime: now },
  };
}
