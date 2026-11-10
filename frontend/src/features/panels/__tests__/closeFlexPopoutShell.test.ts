import { describe, it, expect } from 'vitest';
import { matchFlexPopoutLabelByPosition } from '../closeFlexPopoutShell';

describe('matchFlexPopoutLabelByPosition', () => {
  it('matches the popout whose outer origin is nearest within tolerance', () => {
    const label = matchFlexPopoutLabelByPosition(
      { screenX: 100, screenY: 200 },
      [
        { label: 'flex-popout-1', x: 400, y: 400 },
        { label: 'flex-popout-2', x: 102, y: 198 },
      ],
    );
    expect(label).toBe('flex-popout-2');
  });

  it('returns the sole candidate when position misses (orphan shell)', () => {
    expect(
      matchFlexPopoutLabelByPosition(
        { screenX: 0, screenY: 0 },
        [{ label: 'flex-popout-1', x: 500, y: 500 }],
      ),
    ).toBe('flex-popout-1');
  });

  it('returns null when multiple candidates miss (do not close siblings)', () => {
    expect(
      matchFlexPopoutLabelByPosition(
        { screenX: 0, screenY: 0 },
        [
          { label: 'flex-popout-1', x: 500, y: 500 },
          { label: 'flex-popout-2', x: 800, y: 200 },
        ],
      ),
    ).toBeNull();
  });
});
