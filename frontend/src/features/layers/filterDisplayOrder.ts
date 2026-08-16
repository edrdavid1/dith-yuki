/**
 * Layers panel order is the document order: `filters[0]` is the top row
 * (most final). Image Source stays at the bottom. The engine applies
 * `filters` from last to first so the top row sees the row below, not Raw.
 */

export function displayFilterOrder<T>(stack: T[]): T[] {
  return stack;
}

/**
 * Display index == storage index. Drop-before `dropDisplayIndex`
 * (`stackLen` = just above Image Source).
 */
export function stackIndexAfterDisplayReorder(
  stackLen: number,
  currentDisplayIndex: number,
  dropDisplayIndex: number,
): number {
  if (stackLen <= 0) return 0;
  let finalDisplay = dropDisplayIndex;
  if (currentDisplayIndex < finalDisplay) {
    finalDisplay -= 1;
  }
  return Math.max(0, Math.min(stackLen - 1, finalDisplay));
}
