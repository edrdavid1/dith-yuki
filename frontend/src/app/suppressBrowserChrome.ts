/**
 * Suppress WebView/browser chrome that reminds users they are in a browser:
 * right-click Reload / Inspect, Cmd/Ctrl+R, F5, DevTools shortcuts.
 *
 * App UI that needs a custom context menu still works — those handlers call
 * preventDefault themselves; we only block the engine's default menu + keys.
 */

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}

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

function onKeyDown(e: KeyboardEvent) {
  if (isBrowserInspectKey(e)) {
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (isBrowserReloadKey(e) && !isEditableTarget(e.target)) {
    e.preventDefault();
    e.stopPropagation();
  }
}

function onContextMenu(e: MouseEvent) {
  // Allow OS edit menus in fields; block WebView Reload / Inspect elsewhere.
  if (isEditableTarget(e.target)) return;
  e.preventDefault();
}

/** Install once per document. Returns cleanup. */
export function suppressBrowserChrome(doc: Document = document): () => void {
  doc.addEventListener('keydown', onKeyDown, true);
  doc.addEventListener('contextmenu', onContextMenu, true);
  return () => {
    doc.removeEventListener('keydown', onKeyDown, true);
    doc.removeEventListener('contextmenu', onContextMenu, true);
  };
}
