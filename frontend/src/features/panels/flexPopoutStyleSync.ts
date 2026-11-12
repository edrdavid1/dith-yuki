/**
 * W5: keep FlexLayout popout documents in sync with the main <head> and
 * root theme attributes (HMR style injection, CSS variables, data-*).
 *
 * Initial style copy still happens in the FloatingWindow patch; this module
 * mirrors subsequent head mutations into registered popout documents.
 */

const STYLE_MARKER = 'data-flex-dom-styles';
const ROOT_SYNC_ATTRS = ['class', 'style', 'lang', 'dir'] as const;

type PopoutEntry = {
  doc: Document;
  win: Window;
};

const popouts = new Map<object, PopoutEntry>();
let headObserver: MutationObserver | null = null;
let rootObserver: MutationObserver | null = null;
let mediaQuery: MediaQueryList | null = null;
let mediaHandler: ((ev: MediaQueryListEvent) => void) | null = null;
let started = false;

/** Register a popout browsing context (FlexPopoutChrome mount). */
export function registerFlexPopoutDocument(doc: Document): () => void {
  const win = doc.defaultView;
  if (!win || win === window || win.closed) {
    return () => undefined;
  }
  const key = doc as unknown as object;
  popouts.set(key, { doc, win });
  // Immediate sync so late-mounted chrome picks up current head / theme.
  syncOne(doc);
  ensureObservers();
  return () => {
    popouts.delete(key);
    if (popouts.size === 0 && !started) stopObservers();
  };
}

/** Test helper. */
export function registeredFlexPopoutCountForTests(): number {
  return popouts.size;
}

/** Test helper — clear registry + observers. */
export function resetFlexPopoutStyleSyncForTests(): void {
  popouts.clear();
  stopObservers();
  started = false;
}

export function startFlexPopoutStyleSync(): () => void {
  if (started) {
    return () => undefined;
  }
  started = true;
  ensureObservers();
  syncAll();
  return () => {
    started = false;
    if (popouts.size === 0) stopObservers();
  };
}

function ensureObservers(): void {
  if (typeof MutationObserver === 'undefined') return;
  const head = document.head;
  if (!head) return;

  if (!headObserver) {
    headObserver = new MutationObserver(() => {
      syncAll();
    });
    headObserver.observe(head, {
      childList: true,
      subtree: true,
      characterData: true,
      attributes: true,
    });
  }

  if (!rootObserver) {
    rootObserver = new MutationObserver(() => syncThemeAttrsAll());
    rootObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: [...ROOT_SYNC_ATTRS, 'data-platform', 'data-theme'],
    });
    if (document.body) {
      rootObserver.observe(document.body, {
        attributes: true,
        attributeFilter: [...ROOT_SYNC_ATTRS],
      });
    }
  }

  if (!mediaQuery && typeof window.matchMedia === 'function') {
    try {
      mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
      mediaHandler = () => syncThemeAttrsAll();
      mediaQuery.addEventListener('change', mediaHandler);
    } catch (err) {
      console.error('[flex-dock] prefers-color-scheme listen failed', err);
    }
  }
}

function stopObservers(): void {
  headObserver?.disconnect();
  headObserver = null;
  rootObserver?.disconnect();
  rootObserver = null;
  if (mediaQuery && mediaHandler) {
    mediaQuery.removeEventListener('change', mediaHandler);
  }
  mediaQuery = null;
  mediaHandler = null;
}

function syncAll(): void {
  for (const { doc, win } of popouts.values()) {
    if (win.closed) continue;
    syncOne(doc);
  }
}

function syncThemeAttrsAll(): void {
  for (const { doc, win } of popouts.values()) {
    if (win.closed) continue;
    syncThemeAttrs(doc);
  }
}

/** Pure: mirror main head style/link nodes into dest (idempotent replace). */
export function copyDomStylesTo(destDoc: Document, srcDoc: Document = document): void {
  if (!destDoc.head) return;
  destDoc.head.querySelectorAll(`[${STYLE_MARKER}]`).forEach((n) => n.remove());

  const nodes = srcDoc.head.querySelectorAll('style, link[rel="stylesheet"]');
  nodes.forEach((node) => {
    const clone = node.cloneNode(true) as HTMLElement;
    clone.setAttribute(STYLE_MARKER, '1');
    if (clone.tagName === 'LINK' && clone.getAttribute('href')) {
      try {
        const base =
          srcDoc.defaultView?.location.href ??
          (typeof location !== 'undefined' ? location.href : 'http://localhost/');
        clone.setAttribute('href', new URL(clone.getAttribute('href')!, base).href);
      } catch {
        /* keep relative */
      }
    }
    destDoc.head!.appendChild(clone);
  });
}

/** Pure: mirror html/body theme-related attributes + color-scheme. */
export function syncThemeAttrs(destDoc: Document, srcDoc: Document = document): void {
  mirrorElementAttrs(srcDoc.documentElement, destDoc.documentElement);
  if (srcDoc.body && destDoc.body) {
    mirrorElementAttrs(srcDoc.body, destDoc.body);
  }

  try {
    if (typeof window.matchMedia === 'function') {
      const scheme = window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light';
      destDoc.documentElement.style.colorScheme = scheme;
    }
  } catch {
    /* jsdom */
  }
}

function mirrorElementAttrs(src: Element, dest: Element): void {
  for (const name of ROOT_SYNC_ATTRS) {
    const v = src.getAttribute(name);
    if (v == null || v === '') dest.removeAttribute(name);
    else dest.setAttribute(name, v);
  }
  for (const attr of Array.from(src.attributes)) {
    if (!attr.name.startsWith('data-')) continue;
    dest.setAttribute(attr.name, attr.value);
  }
}

function syncOne(destDoc: Document): void {
  try {
    copyDomStylesTo(destDoc);
    syncThemeAttrs(destDoc);
  } catch (err) {
    console.error('[flex-dock] flexPopoutStyleSync failed', err);
  }
}
