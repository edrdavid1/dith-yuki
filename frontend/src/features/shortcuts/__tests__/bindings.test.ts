import {
  defaultShortcutMap,
  eventToChord,
  findMatchingShortcut,
  formatChord,
  isEditableKeyboardTarget,
  matchChord,
  parseStoredShortcutMap,
} from '../bindings';

describe('shortcut bindings', () => {
  it('matches Photoshop defaults for new layer and zoom', () => {
    const map = defaultShortcutMap();
    const newLayer = new KeyboardEvent('keydown', { key: 'n', metaKey: true, shiftKey: true });
    const zoomIn = new KeyboardEvent('keydown', { key: '=', metaKey: true });
    const zoomFit = new KeyboardEvent('keydown', { key: '0', ctrlKey: true });
    const del = new KeyboardEvent('keydown', { key: 'Backspace' });
    const focus = new KeyboardEvent('keydown', { key: 'Tab' });

    expect(findMatchingShortcut(newLayer, map)).toBe('newLayer');
    expect(findMatchingShortcut(zoomIn, map)).toBe('zoomIn');
    expect(findMatchingShortcut(zoomFit, map)).toBe('zoomFit');
    expect(findMatchingShortcut(del, map)).toBe('deleteLayer');
    expect(findMatchingShortcut(focus, map)).toBe('focusMode');
  });

  it('does not claim Cmd/Ctrl+C/V/X (system clipboard)', () => {
    const map = defaultShortcutMap();
    for (const key of ['c', 'v', 'x'] as const) {
      expect(
        findMatchingShortcut(new KeyboardEvent('keydown', { key, metaKey: true }), map)
      ).toBeNull();
      expect(
        findMatchingShortcut(new KeyboardEvent('keydown', { key, ctrlKey: true }), map)
      ).toBeNull();
    }
  });

  it('isEditableKeyboardTarget covers inputs and nested fields', () => {
    const input = document.createElement('input');
    expect(isEditableKeyboardTarget(input)).toBe(true);
    const wrap = document.createElement('div');
    const nested = document.createElement('textarea');
    wrap.appendChild(nested);
    expect(isEditableKeyboardTarget(nested)).toBe(true);
    expect(isEditableKeyboardTarget(document.createElement('div'))).toBe(false);
  });

  it('formats chords for Windows-style Ctrl labels', () => {
    expect(formatChord({ key: 'z', mod: true, alt: false, shift: true }, false)).toBe(
      'Ctrl+Shift+Z'
    );
  });

  it('round-trips stored maps and ignores junk', () => {
    const parsed = parseStoredShortcutMap({
      undo: [{ key: 'u', mod: true, alt: false, shift: false }],
      nope: [{ key: 'x' }],
    });
    expect(parsed.undo).toEqual([{ key: 'u', mod: true, alt: false, shift: false }]);
    expect(parsed.redo).toEqual(defaultShortcutMap().redo);
  });

  it('eventToChord treats meta and ctrl as mod', () => {
    const e = new KeyboardEvent('keydown', { key: 'S', ctrlKey: true, shiftKey: true });
    const chord = eventToChord(e);
    expect(chord).toEqual({ key: 's', mod: true, alt: false, shift: true });
    expect(matchChord(e, chord)).toBe(true);
  });
});
