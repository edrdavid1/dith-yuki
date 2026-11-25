/**
 * Copy for the pinned-memory toast (T3). Backend emits `memory-warning` when
 * non-evictable tiles (open Raw + Dithered) exceed the RAM budget; hysteresis
 * is enforced there (re-fire only after drop below 90%).
 */
export const OPEN_DOC_MEMORY_WARNING =
  'Many large documents are open — source pixels stay in RAM. Close unused tabs if the app feels slow.';

/** @deprecated Prefer the backend `memory-warning` event; kept for unit tests of tab-count UX. */
export const OPEN_DOC_MEMORY_WARN_AT = 3;

let lastWarnedAtCount = 0;

/**
 * Legacy tab-count heuristic (tests). Production toast comes from `memory-warning`.
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

/** Test-only reset. */
export function resetOpenDocMemoryWarnState(): void {
  lastWarnedAtCount = 0;
}
