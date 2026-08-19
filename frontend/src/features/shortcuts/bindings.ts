import { isMacOS } from '../../lib/platform';

export type ShortcutId =
  | 'newProject'
  | 'openImage'
  | 'openProject'
  | 'saveProject'
  | 'saveProjectAs'
  | 'undo'
  | 'redo'
  | 'newLayer'
  | 'duplicateLayer'
  | 'deleteLayer'
  | 'zoomIn'
  | 'zoomOut'
  | 'zoomFit'
  | 'zoomActual'
  | 'preferences'
  | 'focusMode';

export type Chord = {
  key: string;
  mod: boolean;
  alt: boolean;
  shift: boolean;
};

export type ShortcutMap = Record<ShortcutId, Chord[]>;

export const SHORTCUT_LABELS: Record<ShortcutId, string> = {
  newProject: 'New Project',
  openImage: 'Open Image',
  openProject: 'Open Project',
  saveProject: 'Save Project',
  saveProjectAs: 'Save Project As',
  undo: 'Undo',
  redo: 'Redo',
  newLayer: 'New Layer / Add Effect',
  duplicateLayer: 'Duplicate Layer',
  deleteLayer: 'Delete Layer / Effect',
  zoomIn: 'Zoom In',
  zoomOut: 'Zoom Out',
  zoomFit: 'Fit on Screen',
  zoomActual: 'Actual Pixels (100%)',
  preferences: 'Preferences',
  focusMode: 'Focus Mode',
};

export const SHORTCUT_IDS: ShortcutId[] = [
  'newProject',
  'openImage',
  'openProject',
  'saveProject',
  'saveProjectAs',
  'undo',
  'redo',
  'newLayer',
  'duplicateLayer',
  'deleteLayer',
  'zoomIn',
  'zoomOut',
  'zoomFit',
  'zoomActual',
  'preferences',
  'focusMode',
];

function chord(key: string, opts?: { mod?: boolean; alt?: boolean; shift?: boolean }): Chord {
  return {
    key,
    mod: opts?.mod ?? false,
    alt: opts?.alt ?? false,
    shift: opts?.shift ?? false,
  };
}

/** Photoshop-style defaults. `mod` is ⌘ on macOS and Ctrl elsewhere. */
export function defaultShortcutMap(): ShortcutMap {
  return {
    newProject: [chord('n', { mod: true })],
    openImage: [chord('o', { mod: true })],
    openProject: [chord('o', { mod: true, shift: true })],
    saveProject: [chord('s', { mod: true })],
    saveProjectAs: [chord('s', { mod: true, shift: true })],
    undo: [chord('z', { mod: true })],
    redo: [chord('z', { mod: true, shift: true })],
    newLayer: [chord('n', { mod: true, shift: true })],
    duplicateLayer: [chord('j', { mod: true })],
    deleteLayer: [chord('Backspace'), chord('Delete')],
    zoomIn: [chord('=', { mod: true }), chord('=', { mod: true, shift: true }), chord('+', { mod: true })],
    zoomOut: [chord('-', { mod: true })],
    zoomFit: [chord('0', { mod: true })],
    zoomActual: [chord('1', { mod: true })],
    preferences: [chord(',', { mod: true })],
    focusMode: [chord('Tab')],
  };
}

export function normalizeKey(key: string): string {
  if (key === ' ') return 'Space';
  if (key.length === 1) return key.toLowerCase();
  return key;
}

export function eventToChord(e: KeyboardEvent): Chord {
  return {
    key: normalizeKey(e.key),
    mod: e.metaKey || e.ctrlKey,
    alt: e.altKey,
    shift: e.shiftKey,
  };
}

export function chordsEqual(a: Chord, b: Chord): boolean {
  return a.key === b.key && a.mod === b.mod && a.alt === b.alt && a.shift === b.shift;
}

export function matchChord(event: KeyboardEvent, chord: Chord): boolean {
  return chordsEqual(eventToChord(event), chord);
}

export function findMatchingShortcut(event: KeyboardEvent, map: ShortcutMap): ShortcutId | null {
  for (const id of SHORTCUT_IDS) {
    if (map[id].some((c) => matchChord(event, c))) return id;
  }
  return null;
}

export function formatChord(c: Chord, mac = isMacOS()): string {
  const keyLabel = formatKey(c.key, mac);
  if (mac) {
    return `${c.alt ? '⌥' : ''}${c.shift ? '⇧' : ''}${c.mod ? '⌘' : ''}${keyLabel}`;
  }
  const parts: string[] = [];
  if (c.mod) parts.push('Ctrl');
  if (c.alt) parts.push('Alt');
  if (c.shift) parts.push('Shift');
  parts.push(keyLabel);
  return parts.join('+');
}

function formatKey(key: string, mac: boolean): string {
  switch (key) {
    case 'Backspace':
      return mac ? '⌫' : 'Backspace';
    case 'Delete':
      return mac ? '⌦' : 'Delete';
    case 'Escape':
      return mac ? 'esc' : 'Esc';
    case 'Tab':
      return mac ? '⇥' : 'Tab';
    case ' ':
    case 'Space':
      return 'Space';
    case ',':
      return ',';
    case '=':
      return '=';
    case '+':
      return '+';
    case '-':
      return '−';
    default:
      return key.length === 1 ? key.toUpperCase() : key;
  }
}

export function formatChords(chords: Chord[], mac = isMacOS()): string {
  return chords.map((c) => formatChord(c, mac)).join(' / ');
}

export function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'));
}

export function parseStoredShortcutMap(raw: unknown): ShortcutMap {
  const defaults = defaultShortcutMap();
  if (!raw || typeof raw !== 'object') return defaults;
  const obj = raw as Record<string, unknown>;
  const next = { ...defaults };
  for (const id of SHORTCUT_IDS) {
    const value = obj[id];
    if (!Array.isArray(value)) continue;
    const chords: Chord[] = [];
    for (const item of value) {
      if (!item || typeof item !== 'object') continue;
      const rec = item as Record<string, unknown>;
      if (typeof rec.key !== 'string' || rec.key.length === 0) continue;
      chords.push({
        key: normalizeKey(rec.key),
        mod: Boolean(rec.mod),
        alt: Boolean(rec.alt),
        shift: Boolean(rec.shift),
      });
    }
    if (chords.length > 0) next[id] = chords;
  }
  return next;
}

export const SHORTCUTS_STORAGE_KEY = 'dither.shortcuts';
