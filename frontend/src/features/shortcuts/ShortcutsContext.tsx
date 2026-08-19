import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  SHORTCUTS_STORAGE_KEY,
  defaultShortcutMap,
  parseStoredShortcutMap,
  type Chord,
  type ShortcutId,
  type ShortcutMap,
} from './bindings';

type ShortcutsContextValue = {
  bindings: ShortcutMap;
  capturing: ShortcutId | null;
  setCapturing: (id: ShortcutId | null) => void;
  setBinding: (id: ShortcutId, chords: Chord[]) => void;
  resetDefaults: () => void;
};

const ShortcutsContext = createContext<ShortcutsContextValue | null>(null);

function readStored(): ShortcutMap {
  try {
    if (typeof localStorage === 'undefined') return defaultShortcutMap();
    const raw = localStorage.getItem(SHORTCUTS_STORAGE_KEY);
    if (!raw) return defaultShortcutMap();
    return parseStoredShortcutMap(JSON.parse(raw));
  } catch {
    return defaultShortcutMap();
  }
}

function writeStored(map: ShortcutMap): void {
  try {
    localStorage.setItem(SHORTCUTS_STORAGE_KEY, JSON.stringify(map));
  } catch {
    /* ignore quota / private mode */
  }
}

function withoutChord(map: ShortcutMap, exceptId: ShortcutId, chord: Chord): ShortcutMap {
  const next = { ...map };
  for (const id of Object.keys(next) as ShortcutId[]) {
    if (id === exceptId) continue;
    next[id] = next[id].filter(
      (c) => !(c.key === chord.key && c.mod === chord.mod && c.alt === chord.alt && c.shift === chord.shift)
    );
  }
  return next;
}

export function ShortcutsProvider({ children }: { children: ReactNode }) {
  const [bindings, setBindings] = useState<ShortcutMap>(readStored);
  const [capturing, setCapturing] = useState<ShortcutId | null>(null);

  const setBinding = useCallback((id: ShortcutId, chords: Chord[]) => {
    setBindings((prev) => {
      let next: ShortcutMap = { ...prev, [id]: chords };
      for (const chord of chords) {
        next = withoutChord(next, id, chord);
      }
      writeStored(next);
      return next;
    });
  }, []);

  const resetDefaults = useCallback(() => {
    const defaults = defaultShortcutMap();
    writeStored(defaults);
    setBindings(defaults);
  }, []);

  const value = useMemo(
    () => ({ bindings, capturing, setCapturing, setBinding, resetDefaults }),
    [bindings, capturing, setBinding, resetDefaults]
  );

  return <ShortcutsContext.Provider value={value}>{children}</ShortcutsContext.Provider>;
}

export function useShortcuts(): ShortcutsContextValue {
  const ctx = useContext(ShortcutsContext);
  if (!ctx) {
    throw new Error('useShortcuts must be used within ShortcutsProvider');
  }
  return ctx;
}

export function useShortcutBindings(): ShortcutMap {
  const ctx = useContext(ShortcutsContext);
  return ctx?.bindings ?? defaultShortcutMap();
}
