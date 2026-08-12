import { describe, it, expect } from 'vitest';
import { sidebarEffectiveWidth } from '../../features/panels/DockedSidebar';

describe('sidebarEffectiveWidth', () => {
  it('both sides occupied use preferred width when expanded', () => {
    expect(sidebarEffectiveWidth(2, false, 332)).toBe(332);
    expect(sidebarEffectiveWidth(1, false, 280)).toBe(280);
  });

  it('collapsed strip is 40px when panels exist', () => {
    expect(sidebarEffectiveWidth(1, true, 332)).toBe(40);
    expect(sidebarEffectiveWidth(3, true, 500)).toBe(40);
  });

  it('empty side is 0 regardless of collapsed/width prefs', () => {
    expect(sidebarEffectiveWidth(0, false, 332)).toBe(0);
    expect(sidebarEffectiveWidth(0, true, 332)).toBe(0);
  });
});
