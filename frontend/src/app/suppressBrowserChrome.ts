/**
 * Suppress WebView/browser chrome that reminds users they are in a browser:
 * right-click Reload / Inspect, Cmd/Ctrl+R, F5, DevTools shortcuts.
 *
 * App UI that needs a custom context menu still works — those handlers call
 * preventDefault themselves; we only block the engine's default menu + keys.
 */

import { isEditableKeyboardTarget } from '../features/shortcuts/bindings';

function isBrowserReloadKey(e: KeyboardEvent): boolean {
  const key = e.key;
  if (key === 'F5') return true;
  const mod = e.metaKey || e.ctrlKey;
  if (!mod) return false;
  // Cmd/Ctrl+R, Cmd/Ctrl+Shift+R
  return key === 'r' || key === 'R';
}

function isBrowserInspectKey(e: KeyboardEvent): boolean {
  if (e.key === 'F12') return true;
  // Cmd+Option+I (macOS) / Ctrl+Shift+I (Windows/Linux)
  if ((e.key === 'i' || e.key === 'I') && (e.metaKey || e.ctrlKey) && (e.altKey || e.shiftKey)) {
    return true;
  }
  // Ctrl+Shift+J / Cmd+Option+J (console)
  if ((e.key === 'j' || e.key === 'J') && (e.metaKey || e.ctrlKey) && (e.altKey || e.shiftKey)) {
    return true;
  }
  // Ctrl+Shift+C (inspect element)
  if ((e.key === 'c' || e.key === 'C') && e.ctrlKey && e.shiftKey && !e.metaKey) {
    return true;
  }
  return false;
}

function onKeyDown(e: KeyboardEvent, devtools: boolean) {
  if (!devtools && isBrowserInspectKey(e)) {
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (isBrowserReloadKey(e) && !isEditableKeyboardTarget(e.target)) {
    e.preventDefault();
    e.stopPropagation();
  }
}

function onContextMenu(e: MouseEvent, devtools: boolean) {
  // Allow OS edit menus in fields; block WebView Reload / Inspect elsewhere.
  // Dev mode leaves the menu so Inspect Element can open.
  if (devtools || isEditableKeyboardTarget(e.target)) return;
  e.preventDefault();
}

/** Install once per document. Returns cleanup. `devtools` unlocks Inspect. */
export function suppressBrowserChrome(
  doc: Document = document,
  devtools = false,
): () => void {
  const keydown = (e: KeyboardEvent) => onKeyDown(e, devtools);
  const contextmenu = (e: MouseEvent) => onContextMenu(e, devtools);
  doc.addEventListener('keydown', keydown, true);
  doc.addEventListener('contextmenu', contextmenu, true);
  return () => {
    doc.removeEventListener('keydown', keydown, true);
    doc.removeEventListener('contextmenu', contextmenu, true);
  };
}
