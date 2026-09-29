import { describe, expect, it } from 'vitest';
import {
  MIN_INTERVAL_MS,
  createHapticMotionState,
  evaluateSliderHaptic,
} from '../sliderHaptics';

describe('sliderHaptics (value-change + throttle)', () => {
  it('exports MIN_INTERVAL_MS = 40', () => {
    expect(MIN_INTERVAL_MS).toBe(40);
  });

  it('fires LevelChange when value changes and throttle allows', () => {
    const state = createHapticMotionState(0);
    const d = evaluateSliderHaptic({
      prev: 0,
      next: 1,
      min: 0,
      max: 100,
      now: 1000,
      state,
    });
    expect(d).toEqual({
      trigger: true,
      pattern: 'levelChange',
      state: { lastHapticValue: 1, lastHapticTime: 1000 },
    });
  });

  it('does not fire when value is unchanged', () => {
    const d = evaluateSliderHaptic({
      prev: 5,
      next: 5,
      min: 0,
      max: 100,
      now: 1000,
      state: createHapticMotionState(5),
    });
    expect(d.trigger).toBe(false);
  });

  it('does not fire when next equals lastHapticValue', () => {
    const d = evaluateSliderHaptic({
      prev: 4,
      next: 5,
      min: 0,
      max: 100,
      now: 2000,
      state: { lastHapticValue: 5, lastHapticTime: 1000 },
    });
    expect(d.trigger).toBe(false);
  });

  it('throttles LevelChange to MIN_INTERVAL_MS without advancing lastHapticValue', () => {
    const state = { lastHapticValue: 0, lastHapticTime: 1000 };
    const d = evaluateSliderHaptic({
      prev: 0,
      next: 1,
      min: 0,
      max: 100,
      now: 1000 + MIN_INTERVAL_MS - 1,
      state,
    });
    expect(d.trigger).toBe(false);
    expect(d.state.lastHapticValue).toBe(0);

    const d2 = evaluateSliderHaptic({
      prev: 1,
      next: 2,
      min: 0,
      max: 100,
      now: 1000 + MIN_INTERVAL_MS,
      state: d.state,
    });
    expect(d2.trigger).toBe(true);
    expect(d2.pattern).toBe('levelChange');
    expect(d2.state.lastHapticValue).toBe(2);
  });

  it('fires Generic on min/max regardless of throttle', () => {
    const state = { lastHapticValue: 99, lastHapticTime: 1000 };
    const d = evaluateSliderHaptic({
      prev: 99,
      next: 100,
      min: 0,
      max: 100,
      now: 1001, // well inside throttle window
      state,
    });
    expect(d.trigger).toBe(true);
    expect(d.pattern).toBe('generic');
    expect(d.state.lastHapticValue).toBe(100);
  });

  it('fires one LevelChange per distinct unit when spaced ≥ 40 ms', () => {
    let state = createHapticMotionState(0);
    let now = 1000;
    for (let v = 1; v <= 5; v++) {
      const d = evaluateSliderHaptic({
        prev: v - 1,
        next: v,
        min: 0,
        max: 100,
        now,
        state,
      });
      expect(d.trigger).toBe(true);
      expect(d.pattern).toBe('levelChange');
      state = d.state;
      now += MIN_INTERVAL_MS;
    }
  });
});
