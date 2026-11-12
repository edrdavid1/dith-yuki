import { describe, it, expect } from 'vitest';
import { ownerDocumentOf, ownerWindowOf } from '../useOwnerDocument';

describe('useOwnerDocument helpers (W5)', () => {
  it('ownerDocumentOf falls back to document for null', () => {
    expect(ownerDocumentOf(null)).toBe(document);
  });

  it('ownerDocumentOf returns the node’s document', () => {
    const el = document.createElement('div');
    document.body.appendChild(el);
    expect(ownerDocumentOf(el)).toBe(document);
    el.remove();
  });

  it('ownerWindowOf returns defaultView', () => {
    expect(ownerWindowOf(document.body)).toBe(window);
  });
});
