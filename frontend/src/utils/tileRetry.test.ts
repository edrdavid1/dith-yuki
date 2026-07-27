import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { TileRetryManager } from './tileRetry';

describe('TileRetryManager', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('schedules a retry on first failure', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    const scheduled = manager.recordFailure('0/3/2');
    expect(scheduled).toBe(true);
    expect(onRetry).not.toHaveBeenCalled();

    vi.advanceTimersByTime(500);
    expect(onRetry).toHaveBeenCalledWith('0/3/2');
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('allows up to 2 retries (3 total attempts)', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    // First failure → schedules retry 1
    expect(manager.recordFailure('0/1/1')).toBe(true);
    vi.advanceTimersByTime(500);
    expect(onRetry).toHaveBeenCalledTimes(1);

    // Second failure (after retry 1 failed) → schedules retry 2
    expect(manager.recordFailure('0/1/1')).toBe(true);
    vi.advanceTimersByTime(500);
    expect(onRetry).toHaveBeenCalledTimes(2);

    // Third failure (after retry 2 failed) → permanently failed
    expect(manager.recordFailure('0/1/1')).toBe(false);
    expect(manager.isPermanentlyFailed('0/1/1')).toBe(true);
  });

  it('uses 500ms delay between retries', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    manager.recordFailure('0/0/0');

    // Not called before 500ms
    vi.advanceTimersByTime(499);
    expect(onRetry).not.toHaveBeenCalled();

    // Called at exactly 500ms
    vi.advanceTimersByTime(1);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('reports isPermanentlyFailed correctly', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    expect(manager.isPermanentlyFailed('0/0/0')).toBe(false);

    manager.recordFailure('0/0/0');
    expect(manager.isPermanentlyFailed('0/0/0')).toBe(false);

    vi.advanceTimersByTime(500);
    manager.recordFailure('0/0/0');
    expect(manager.isPermanentlyFailed('0/0/0')).toBe(false);

    vi.advanceTimersByTime(500);
    manager.recordFailure('0/0/0');
    expect(manager.isPermanentlyFailed('0/0/0')).toBe(true);
  });

  it('clearFailure removes failure state and cancels timer', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    manager.recordFailure('0/2/3');
    manager.clearFailure('0/2/3');

    // Timer should have been cancelled
    vi.advanceTimersByTime(1000);
    expect(onRetry).not.toHaveBeenCalled();
    expect(manager.hasFailed('0/2/3')).toBe(false);
  });

  it('reset clears all state and cancels timers', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    manager.recordFailure('0/0/0');
    manager.recordFailure('0/1/1');

    manager.reset();

    vi.advanceTimersByTime(1000);
    expect(onRetry).not.toHaveBeenCalled();
    expect(manager.hasFailed('0/0/0')).toBe(false);
    expect(manager.hasFailed('0/1/1')).toBe(false);
  });

  it('cancelAll stops timers but preserves failure state', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    manager.recordFailure('0/5/5');
    manager.cancelAll();

    vi.advanceTimersByTime(1000);
    expect(onRetry).not.toHaveBeenCalled();
    // Failure state preserved
    expect(manager.hasFailed('0/5/5')).toBe(true);
  });

  it('tracks retry count correctly', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    expect(manager.getRetryCount('0/0/0')).toBe(0);

    manager.recordFailure('0/0/0');
    expect(manager.getRetryCount('0/0/0')).toBe(0); // timer pending

    vi.advanceTimersByTime(500);
    expect(manager.getRetryCount('0/0/0')).toBe(1); // first retry fired

    manager.recordFailure('0/0/0');
    vi.advanceTimersByTime(500);
    expect(manager.getRetryCount('0/0/0')).toBe(2); // second retry fired
  });

  it('handles multiple tile keys independently', () => {
    const onRetry = vi.fn();
    const manager = new TileRetryManager(onRetry);

    manager.recordFailure('0/0/0');
    manager.recordFailure('0/1/1');

    vi.advanceTimersByTime(500);
    expect(onRetry).toHaveBeenCalledWith('0/0/0');
    expect(onRetry).toHaveBeenCalledWith('0/1/1');
    expect(onRetry).toHaveBeenCalledTimes(2);

    // Permanently fail one but not the other
    manager.recordFailure('0/0/0');
    vi.advanceTimersByTime(500);
    manager.recordFailure('0/0/0'); // 3rd failure → permanent

    expect(manager.isPermanentlyFailed('0/0/0')).toBe(true);
    expect(manager.isPermanentlyFailed('0/1/1')).toBe(false);
  });
});
