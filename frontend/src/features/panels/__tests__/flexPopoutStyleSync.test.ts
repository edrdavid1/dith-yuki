import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  copyDomStylesTo,
  syncThemeAttrs,
  resetFlexPopoutStyleSyncForTests,
} from '../flexPopoutStyleSync';

describe('flexPopoutStyleSync (W5)', () => {
  beforeEach(() => {
    resetFlexPopoutStyleSyncForTests();
  });

  afterEach(() => {
    resetFlexPopoutStyleSyncForTests();
  });

  it('copies style and stylesheet link nodes with marker', () => {
    const style = document.createElement('style');
    style.textContent = '.probe{color:red}';
    document.head.appendChild(style);

    const dest = document.implementation.createHTMLDocument('popout');
    copyDomStylesTo(dest, document);

    const cloned = dest.head.querySelectorAll('[data-flex-dom-styles]');
    expect(cloned.length).toBeGreaterThan(0);
    expect(
      Array.from(cloned).some((n) => n.textContent?.includes('.probe')),
    ).toBe(true);

    style.remove();
  });

  it('mirrors html data-* and class onto the popout document', () => {
    document.documentElement.dataset.platform = 'macos';
    document.documentElement.classList.add('is-panel');

    const dest = document.implementation.createHTMLDocument('popout');
    dest.body = dest.createElement('body');
    syncThemeAttrs(dest, document);

    expect(dest.documentElement.getAttribute('data-platform')).toBe('macos');
    expect(dest.documentElement.classList.contains('is-panel')).toBe(true);

    document.documentElement.classList.remove('is-panel');
    delete document.documentElement.dataset.platform;
  });
});
