import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { suppressBrowserChrome } from '../suppressBrowserChrome';

describe('suppressBrowserChrome', () => {
  let cleanup: (() => void) | undefined;

  beforeEach(() => {
    cleanup = suppressBrowserChrome(document);
  });

  afterEach(() => {
    cleanup?.();
  });

  it('prevents Cmd/Ctrl+R reload', () => {
    const e = new KeyboardEvent('keydown', {
      key: 'r',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    const prevented = !document.dispatchEvent(e) || e.defaultPrevented;
    expect(prevented).toBe(true);
  });

  it('prevents F12 inspect', () => {
    const e = new KeyboardEvent('keydown', {
      key: 'F12',
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
  });

  it('prevents default context menu (Reload / Inspect)', () => {
    const e = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
  });
});
