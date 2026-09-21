import { describe, expect, it } from 'vitest';
import {
  dockBottomCornerFromEdges,
  markDockBottomCorner,
} from '../markDockBottomCorner';

describe('dockBottomCornerFromEdges', () => {
  it('returns right for bottom+right', () => {
    expect(dockBottomCornerFromEdges('bottom right')).toBe('right');
  });

  it('returns left for bottom+left', () => {
    expect(dockBottomCornerFromEdges('bottom left')).toBe('left');
  });

  it('returns null without bottom', () => {
    expect(dockBottomCornerFromEdges('left right')).toBeNull();
  });
});

describe('markDockBottomCorner', () => {
  it('marks the bottom tabset and its flush tab chrome (sibling overlay)', () => {
    const root = document.createElement('div');
    const upper = document.createElement('div');
    upper.className = 'flexlayout__tabset';
    upper.getBoundingClientRect = () =>
      ({ top: 0, bottom: 100, left: 0, right: 200, width: 200, height: 100 }) as DOMRect;

    const lower = document.createElement('div');
    lower.className = 'flexlayout__tabset';
    lower.getBoundingClientRect = () =>
      ({ top: 100, bottom: 220, left: 0, right: 200, width: 200, height: 120 }) as DOMRect;

    // Tab chrome is a sibling overlay aligned to the lower tabset bottom.
    const tab = document.createElement('div');
    tab.className = 'flexlayout__tab';
    tab.getBoundingClientRect = () =>
      ({ top: 120, bottom: 220, left: 0, right: 200, width: 200, height: 100 }) as DOMRect;

    const strayTab = document.createElement('div');
    strayTab.className = 'flexlayout__tab';
    strayTab.getBoundingClientRect = () =>
      ({ top: 20, bottom: 100, left: 0, right: 200, width: 200, height: 80 }) as DOMRect;

    root.append(upper, lower, strayTab, tab);

    markDockBottomCorner(root, 'right');

    expect(upper.getAttribute('data-dock-bottom-corner')).toBeNull();
    expect(lower.getAttribute('data-dock-bottom-corner')).toBe('right');
    expect(tab.getAttribute('data-dock-bottom-corner')).toBe('right');
    expect(strayTab.getAttribute('data-dock-bottom-corner')).toBeNull();
  });
});
