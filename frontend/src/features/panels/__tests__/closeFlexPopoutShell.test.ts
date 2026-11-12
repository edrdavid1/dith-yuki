import { describe, it, expect, beforeEach } from 'vitest';
import { matchFlexPopoutLabelByPosition } from '../closeFlexPopoutShell';
import {
  lookupFlexPopoutLabel,
  rememberFlexPopoutLabel,
  resetFlexPopoutRegistryForTests,
} from '../flexPopoutRegistry';

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

  it('returns null on miss even with a sole candidate (W6: no any/first fallback)', () => {
    expect(
      matchFlexPopoutLabelByPosition(
        { screenX: 0, screenY: 0 },
        [{ label: 'flex-popout-1', x: 500, y: 500 }],
      ),
    ).toBeNull();
  });

  it('matches within the widened logical tolerance', () => {
    expect(
      matchFlexPopoutLabelByPosition(
        { screenX: 100, screenY: 200 },
        [{ label: 'flex-popout-1', x: 130, y: 220 }],
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

describe('closeFlexPopoutShell label preference', () => {
  beforeEach(() => {
    resetFlexPopoutRegistryForTests();
  });

  it('registry label survives when position matching would be ambiguous', () => {
    rememberFlexPopoutLabel('colorlab', 'flex-popout-7');
    // Position match among two distant shells would return null — label wins.
    expect(
      matchFlexPopoutLabelByPosition(
        { screenX: 0, screenY: 0 },
        [
          { label: 'flex-popout-1', x: 500, y: 500 },
          { label: 'flex-popout-7', x: 800, y: 200 },
        ],
      ),
    ).toBeNull();
    expect(lookupFlexPopoutLabel('colorlab')).toBe('flex-popout-7');
  });

  it('panelId registry lookup works without a DOM window', () => {
    rememberFlexPopoutLabel('layers', 'flex-popout-3');
    expect(lookupFlexPopoutLabel('layers', null)).toBe('flex-popout-3');
  });
});
