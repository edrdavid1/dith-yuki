/** Soft warning when many documents stay open (Chrome-like). Raw is pinned outside the 512 MiB tile preview cache. */
export const OPEN_DOC_MEMORY_WARN_AT = 3;

let lastWarnedAtCount = 0;

/**
 * Non-blocking notice when open-tab count crosses the threshold upward.
 * Debounced: does not re-fire until count drops below threshold then rises again.
 */
export function shouldWarnOpenDocMemory(tabCount: number): boolean {
  if (tabCount < OPEN_DOC_MEMORY_WARN_AT) {
    lastWarnedAtCount = 0;
    return false;
  }
  if (lastWarnedAtCount >= OPEN_DOC_MEMORY_WARN_AT) {
    return false;
  }
  lastWarnedAtCount = tabCount;
  return true;
}

export const OPEN_DOC_MEMORY_WARNING =
  'Memory use grows with each open document (source pixels stay in RAM). Close unused tabs if the app feels slow or the system is low on memory.';

/** Test-only reset. */
export function resetOpenDocMemoryWarnState(): void {
  lastWarnedAtCount = 0;
}
