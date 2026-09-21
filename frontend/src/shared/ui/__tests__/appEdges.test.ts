import { describe, expect, it } from 'vitest';
import { appEdgesAttr, appEdgesCornerStyle } from '../appEdges';

describe('appEdgesAttr', () => {
  it('joins present edges', () => {
    expect(appEdgesAttr('bottom', 'right')).toBe('bottom right');
  });

  it('skips falsy entries', () => {
    expect(appEdgesAttr('bottom', false && 'left', null, undefined, 'right')).toBe(
      'bottom right',
    );
  });

  it('returns undefined when empty', () => {
    expect(appEdgesAttr()).toBeUndefined();
    expect(appEdgesAttr(false, null)).toBeUndefined();
  });
});

describe('appEdgesCornerStyle', () => {
  it('maps bottom+right to borderBottomRightRadius', () => {
    expect(appEdgesCornerStyle('bottom right')).toEqual({
      borderBottomRightRadius: 'var(--dock-outer-radius)',
    });
  });

  it('maps bottom+left to borderBottomLeftRadius', () => {
    expect(appEdgesCornerStyle('bottom left')).toEqual({
      borderBottomLeftRadius: 'var(--dock-outer-radius)',
    });
  });

  it('returns empty when bottom is missing', () => {
    expect(appEdgesCornerStyle('left right')).toEqual({});
  });
});
