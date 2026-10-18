import { describe, it, expect, beforeEach } from 'vitest';
import {
  checkVisibleInvariant,
  recordDrawn,
  recordHttpStatus,
  recordTileReady,
  resetSeriesRegistry,
} from '../tileGenCanvasRegistry';

describe('checkVisibleInvariant', () => {
  beforeEach(() => {
    resetSeriesRegistry();
  });

  it('flags MISSING when visible tile never drawn', () => {
    const v = checkVisibleInvariant({
      visibleKeys: ['0/0/0'],
      expectedGen: 3,
      level: 0,
    });
    expect(v.some((x) => x.class === 'MISSING' && x.key === '0/0/0')).toBe(true);
  });

  it('flags STALE when drawn gen behind expected', () => {
    recordDrawn('0/0/0', 1);
    const v = checkVisibleInvariant({
      visibleKeys: ['0/0/0'],
      expectedGen: 5,
      level: 0,
    });
    expect(v.some((x) => x.class === 'STALE')).toBe(true);
  });

  it('flags MIXED when level has multiple gens', () => {
    recordDrawn('0/0/0', 4);
    recordDrawn('0/1/0', 5);
    const v = checkVisibleInvariant({
      visibleKeys: ['0/0/0', '0/1/0'],
      expectedGen: 5,
      level: 0,
    });
    expect(v.some((x) => x.class === 'MIXED')).toBe(true);
  });

  it('flags STUCK_202 when only 202 responses', () => {
    recordDrawn('0/0/0', 5);
    recordHttpStatus('0/0/0', 202);
    const v = checkVisibleInvariant({
      visibleKeys: ['0/0/0'],
      expectedGen: 5,
      level: 0,
    });
    expect(v.some((x) => x.class === 'STUCK_202')).toBe(true);
  });

  it('flags LOST_EVENT when tileReady without fetch', () => {
    recordDrawn('0/0/0', 5);
    recordTileReady('0/0/0');
    const v = checkVisibleInvariant({
      visibleKeys: ['0/0/0'],
      expectedGen: 5,
      level: 0,
    });
    expect(v.some((x) => x.class === 'LOST_EVENT')).toBe(true);
  });
});
