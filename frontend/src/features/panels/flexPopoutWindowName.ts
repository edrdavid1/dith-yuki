/**
 * FlexLayout tab ids look like `#uuid`. Passing that string as `window.open`'s
 * *name* is unsafe in WKWebView/Tauri: names are truncated at `#`, so every
 * popout collapses to the empty name and the second `window.open` reuses (and
 * navigates) the first OS window — earlier detached panels vanish.
 *
 * Registry / FloatingWindow props still key by the raw tab id; only the
 * browsing-context name is sanitized (keep in sync with
 * `src/patches/flexlayoutFloatingWindow.cjs`).
 */
export function flexPopoutWindowName(tabId: string): string {
  const raw = String(tabId ?? '');
  const stripped = raw.replace(/^#/, '');
  const safe = stripped.replace(/[^a-zA-Z0-9_-]/g, '_') || 'tab';
  return `flex-${safe}`;
}
