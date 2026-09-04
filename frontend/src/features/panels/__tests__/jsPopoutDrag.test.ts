import { describe, expect, it } from 'vitest';
import { jsDragWindowPosition } from '../jsPopoutDrag';

describe('jsDragWindowPosition', () => {
  it('applies cursor delta in logical space', () => {
    const next = jsDragWindowPosition(
      { winX: 100, winY: 200, cursorX: 150, cursorY: 220 },
      180,
      260,
    );
    expect(next).toEqual({ x: 130, y: 240 });
  });

  it('keeps negative-origin secondary monitor positions (left of primary)', () => {
    // Secondary display at x=-1920; window starts there, cursor moves further left.
    const next = jsDragWindowPosition(
      { winX: -1600, winY: 100, cursorX: -1500, cursorY: 120 },
      -1700,
      140,
    );
    expect(next.x).toBe(-1800);
    expect(next.y).toBe(120);
  });

  it('keeps negative Y when secondary is above primary', () => {
    const next = jsDragWindowPosition(
      { winX: 100, winY: -800, cursorX: 120, cursorY: -780 },
      140,
      -900,
    );
    expect(next).toEqual({ x: 120, y: -920 });
  });
});
