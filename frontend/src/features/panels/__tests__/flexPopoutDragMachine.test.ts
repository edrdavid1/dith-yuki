import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  HARD_CAP_MS,
  WATCHDOG_MS,
  applyFinish,
  beginDrag,
  createIdleMachine,
  isCompletingReason,
  settleToIdle,
  type FinishReason,
} from '../flexPopoutDragMachine';

const TERMINATORS: FinishReason[] = [
  'pointerup',
  'buttons-zero',
  'pointercancel',
  'lostpointercapture',
  'blur',
  'visibilityhidden',
  'escape',
  'watchdog',
  'hard-cap',
  'unmount',
  'start-failed',
  'superseded',
];

describe('flexPopoutDragMachine', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts idle', () => {
    expect(createIdleMachine().phase).toBe('idle');
  });

  it('beginDrag moves to dragging', () => {
    const s = beginDrag(createIdleMachine());
    expect(s.phase).toBe('dragging');
    expect(s.finishCount).toBe(0);
  });

  it.each(TERMINATORS)('finish(%s) from dragging reaches a terminal phase then idle', (reason) => {
    let state = beginDrag(createIdleMachine());
    const { next, didFinish, shouldComplete } = applyFinish(state, reason);
    expect(didFinish).toBe(true);
    expect(shouldComplete).toBe(isCompletingReason(reason));
    expect(next.phase).toBe(shouldComplete ? 'completing' : 'cancelled');
    expect(next.finishCount).toBe(1);
    expect(next.lastReason).toBe(reason);
    state = settleToIdle(next);
    expect(state.phase).toBe('idle');
  });

  it('double finish is harmless (idempotent)', () => {
    const dragging = beginDrag(createIdleMachine());
    const first = applyFinish(dragging, 'pointerup');
    expect(first.didFinish).toBe(true);
    const second = applyFinish(first.next, 'pointerup');
    expect(second.didFinish).toBe(false);
    expect(second.next.finishCount).toBe(1);
    expect(second.next.phase).toBe('completing');

    const third = applyFinish(first.next, 'escape');
    expect(third.didFinish).toBe(false);
  });

  it('buttons-zero completes (release outside window)', () => {
    const { shouldComplete, next } = applyFinish(
      beginDrag(createIdleMachine()),
      'buttons-zero',
    );
    expect(shouldComplete).toBe(true);
    expect(next.phase).toBe('completing');
  });

  it('pointerup completes; escape / watchdog / hard-cap cancel', () => {
    expect(isCompletingReason('pointerup')).toBe(true);
    expect(isCompletingReason('escape')).toBe(false);
    expect(isCompletingReason('watchdog')).toBe(false);
    expect(isCompletingReason('hard-cap')).toBe(false);
  });

  it('exports watchdog and hard-cap durations from the spec', () => {
    expect(WATCHDOG_MS).toBe(1500);
    expect(HARD_CAP_MS).toBe(60_000);
  });

  it('finish from idle is a no-op', () => {
    const idle = createIdleMachine();
    const { didFinish, next } = applyFinish(idle, 'pointerup');
    expect(didFinish).toBe(false);
    expect(next.phase).toBe('idle');
  });
});
