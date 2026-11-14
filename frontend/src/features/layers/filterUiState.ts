/**
 * Session-only effect labels. Lives outside the Layers panel so a float/dock
 * remount keeps display numbers and rename text. Not part of the document.
 */

export type DocFilterUiState = {
  numbers: Record<string, number>;
  nextNumber: number;
  names: Record<string, string>;
};

const EMPTY: DocFilterUiState = { numbers: {}, nextNumber: 1, names: {} };

const byDoc = new Map<string, DocFilterUiState>();
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

export function subscribeFilterUi(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getFilterUi(docKey: string): DocFilterUiState {
  return byDoc.get(docKey) ?? EMPTY;
}

/**
 * Assign stable #N values for filters that do not already have one.
 * Ids numbered under another document are skipped so a docId/filter race
 * cannot consume this document's counter.
 */
export function noteFilters(docKey: string, filterIds: readonly string[]): void {
  const claimedElsewhere = new Set<string>();
  for (const [key, ui] of byDoc) {
    if (key === docKey) continue;
    for (const id of Object.keys(ui.numbers)) claimedElsewhere.add(id);
  }

  const current = byDoc.get(docKey) ?? EMPTY;
  let nextNumber = current.nextNumber;
  let changed = false;
  const numbers = { ...current.numbers };
  for (const id of filterIds) {
    if (claimedElsewhere.has(id)) continue;
    if (!(id in numbers)) {
      numbers[id] = nextNumber++;
      changed = true;
    }
  }
  if (!changed) return;
  byDoc.set(docKey, { ...current, numbers, nextNumber });
  emit();
}

export function renameFilter(docKey: string, filterId: string, name: string): void {
  const current = byDoc.get(docKey) ?? EMPTY;
  byDoc.set(docKey, {
    ...current,
    names: { ...current.names, [filterId]: name },
  });
  emit();
}

export function resetFilterUiForTests(): void {
  byDoc.clear();
  emit();
}
