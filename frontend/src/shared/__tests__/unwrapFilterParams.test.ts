import { describe, it, expect } from 'vitest';
import { unwrapFilterParams } from '../unwrapFilterParams';

describe('unwrapFilterParams', () => {
  it('flattens externally tagged Curves params from the engine', () => {
    const flat = unwrapFilterParams({
      Curves: {
        curve: [[0, 0], [0.5, 0.8], [1, 1]],
        channel: 'Red',
      },
    });
    expect(flat.curve).toEqual([[0, 0], [0.5, 0.8], [1, 1]]);
    expect(flat.channel).toBe('Red');
  });

  it('leaves already-flat params alone', () => {
    const src = { curve: [[0, 0], [1, 1]], channel: 'All', type: 'Curves' };
    expect(unwrapFilterParams(src)).toBe(src);
  });

  it('still unwraps DitherV2', () => {
    const flat = unwrapFilterParams({
      DitherV2: { mode: 'floyd_steinberg', levels: 4 },
    });
    expect(flat.mode).toBe('floyd_steinberg');
    expect(flat.levels).toBe(4);
  });

  it('flattens externally tagged Ascii params from the engine', () => {
    const flat = unwrapFilterParams({
      Ascii: {
        font: 'ibm_plex_mono',
        font_px: 14,
        antialias: true,
        contrast: 1.5,
        edge_overlay: true,
      },
    });
    expect(flat.font).toBe('ibm_plex_mono');
    expect(flat.font_px).toBe(14);
    expect(flat.antialias).toBe(true);
    expect(flat.contrast).toBe(1.5);
    expect(flat.edge_overlay).toBe(true);
  });
});
