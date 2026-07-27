/**
 * Tile retry manager — tracks failed tiles and schedules retries.
 *
 * When a tile fetch fails with a non-recoverable error, the retry manager:
 * 1. Records the failure against the tile key
 * 2. Schedules a retry after a 500ms delay
 * 3. Allows up to 2 retries (3 total attempts including the initial)
 * 4. After exhausting retries, marks the tile as permanently failed
 *
 * Requirements: 5.6
 */

/** Maximum number of retry attempts per tile (2 retries = 3 total attempts) */
const MAX_RETRIES = 2;

/** Delay between retry attempts in milliseconds */
const RETRY_DELAY_MS = 500;

export interface TileRetryState {
  /** Number of retries attempted so far */
  retryCount: number;
  /** Whether all retries have been exhausted */
  permanentlyFailed: boolean;
  /** Pending retry timer ID (for cancellation) */
  timerId: ReturnType<typeof setTimeout> | null;
}

export type RetryCallback = (key: string) => void;

/**
 * Manages retry logic for failed tile fetches.
 *
 * Usage:
 *   const retryManager = new TileRetryManager((key) => {
 *     // Re-request the tile identified by `key`
 *     worker.postMessage({ type: 'fetch-tile', ...parseTileKey(key), docId });
 *   });
 *
 *   // On tile error:
 *   retryManager.recordFailure('0/3/2');
 *
 *   // Check if tile has permanently failed:
 *   if (retryManager.isPermanentlyFailed('0/3/2')) {
 *     drawErrorIndicator(...);
 *   }
 */
export class TileRetryManager {
  private failures: Map<string, TileRetryState> = new Map();
  private onRetry: RetryCallback;

  constructor(onRetry: RetryCallback) {
    this.onRetry = onRetry;
  }

  /**
   * Record a tile fetch failure and schedule a retry if attempts remain.
   *
   * @param key - Tile key in "level/x/y" format
   * @returns true if a retry was scheduled, false if permanently failed
   */
  recordFailure(key: string): boolean {
    const state = this.failures.get(key);

    if (!state) {
      // First failure — schedule first retry
      const newState: TileRetryState = {
        retryCount: 0,
        permanentlyFailed: false,
        timerId: null,
      };
      this.failures.set(key, newState);
      this.scheduleRetry(key, newState);
      return true;
    }

    if (state.permanentlyFailed) {
      return false;
    }

    if (state.retryCount >= MAX_RETRIES) {
      // Exhausted all retries
      state.permanentlyFailed = true;
      state.timerId = null;
      return false;
    }

    // Schedule another retry
    this.scheduleRetry(key, state);
    return true;
  }

  /**
   * Check if a tile has permanently failed (all retries exhausted).
   */
  isPermanentlyFailed(key: string): boolean {
    const state = this.failures.get(key);
    return state?.permanentlyFailed ?? false;
  }

  /**
   * Check if a tile has any recorded failure (pending retry or permanent).
   */
  hasFailed(key: string): boolean {
    return this.failures.has(key);
  }

  /**
   * Get the current retry count for a tile.
   */
  getRetryCount(key: string): number {
    return this.failures.get(key)?.retryCount ?? 0;
  }

  /**
   * Clear failure state for a tile (e.g., when it successfully loads after retry).
   */
  clearFailure(key: string): void {
    const state = this.failures.get(key);
    if (state?.timerId !== null && state?.timerId !== undefined) {
      clearTimeout(state.timerId);
    }
    this.failures.delete(key);
  }

  /**
   * Reset all failure tracking state. Cancels pending retry timers.
   */
  reset(): void {
    for (const [, state] of this.failures) {
      if (state.timerId !== null) {
        clearTimeout(state.timerId);
      }
    }
    this.failures.clear();
  }

  /**
   * Cancel all pending retry timers without clearing failure state.
   * Useful when the component is unmounting.
   */
  cancelAll(): void {
    for (const [, state] of this.failures) {
      if (state.timerId !== null) {
        clearTimeout(state.timerId);
        state.timerId = null;
      }
    }
  }

  private scheduleRetry(key: string, state: TileRetryState): void {
    // Clear any existing timer
    if (state.timerId !== null) {
      clearTimeout(state.timerId);
    }

    state.timerId = setTimeout(() => {
      state.retryCount++;
      state.timerId = null;
      this.onRetry(key);
    }, RETRY_DELAY_MS);
  }
}
