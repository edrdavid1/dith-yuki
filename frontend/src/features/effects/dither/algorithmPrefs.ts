/**
 * Persisted favorites / hide prefs for dither algorithm variants.
 * Local-only (same pattern as shell prefs); not part of project files.
 */

import type { AlgorithmVariantId } from './algorithmFamilies';

const PREFS_KEY = 'dither.algorithmPrefs';

export interface AlgorithmPrefs {
  version: 1;
  favorites: AlgorithmVariantId[];
  hidden: AlgorithmVariantId[];
}

function getLocalStorage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

function isVariantId(value: unknown): value is AlgorithmVariantId {
  return typeof value === 'string' && value.length > 0;
}

export function readAlgorithmPrefs(): AlgorithmPrefs {
  try {
    const raw = getLocalStorage()?.getItem(PREFS_KEY);
    if (!raw) return { version: 1, favorites: [], hidden: [] };
    const parsed = JSON.parse(raw) as Partial<AlgorithmPrefs>;
    const favorites = Array.isArray(parsed.favorites)
      ? parsed.favorites.filter(isVariantId)
      : [];
    const hidden = Array.isArray(parsed.hidden) ? parsed.hidden.filter(isVariantId) : [];
    return { version: 1, favorites, hidden };
  } catch {
    return { version: 1, favorites: [], hidden: [] };
  }
}

export function writeAlgorithmPrefs(prefs: AlgorithmPrefs): void {
  try {
    getLocalStorage()?.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // ignore quota / private mode
  }
}

export function toggleFavorite(id: AlgorithmVariantId): AlgorithmPrefs {
  const prefs = readAlgorithmPrefs();
  const set = new Set(prefs.favorites);
  if (set.has(id)) set.delete(id);
  else set.add(id);
  const next = { ...prefs, favorites: Array.from(set) };
  writeAlgorithmPrefs(next);
  return next;
}

export function toggleHidden(id: AlgorithmVariantId): AlgorithmPrefs {
  const prefs = readAlgorithmPrefs();
  const set = new Set(prefs.hidden);
  if (set.has(id)) set.delete(id);
  else set.add(id);
  const next = { ...prefs, hidden: Array.from(set) };
  writeAlgorithmPrefs(next);
  return next;
}

export function isFavorite(prefs: AlgorithmPrefs, id: AlgorithmVariantId): boolean {
  return prefs.favorites.includes(id);
}

export function isHidden(prefs: AlgorithmPrefs, id: AlgorithmVariantId): boolean {
  return prefs.hidden.includes(id);
}
