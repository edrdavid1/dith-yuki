import { describe, it, expect } from 'vitest';
import { sidebarColumnWidth } from '../../features/panels/panelDragMode';

describe('sidebarColumnWidth', () => {
  it('docked expanded uses preferred width', () => {
    expect(sidebarColumnWidth(true, false, 332)).toBe(332);
    expect(sidebarColumnWidth(true, false, 280)).toBe(280);
  });

  it('collapsed strip is 40px when docked', () => {
    expect(sidebarColumnWidth(true, true, 332)).toBe(40);
    expect(sidebarColumnWidth(true, true, 500)).toBe(40);
  });

  it('empty side is 0 regardless of collapsed/width prefs', () => {
    expect(sidebarColumnWidth(false, false, 332)).toBe(0);
    expect(sidebarColumnWidth(false, true, 332)).toBe(0);
  });
});
