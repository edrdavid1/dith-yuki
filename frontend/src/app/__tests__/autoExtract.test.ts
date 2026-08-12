import { describe, it, expect, beforeEach, vi } from 'vitest';
import { extractPalette, maybeAutoExtractPalette } from '../autoExtract';
import { createTestStore } from './testStore';
import { bumpVersion } from '../slices/palettesSlice';

vi.mock('../../shared/ipc', async () => {
  const actual = await vi.importActual<typeof import('../../shared/ipc')>('../../shared/ipc');
  return {
    ...actual,
    generatePalette: vi.fn(),
    logIpcError: vi.fn(),
  };
});

import { generatePalette } from '../../shared/ipc';

const mockGeneratePalette = vi.mocked(generatePalette);

describe('autoExtract', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('extractPalette fills draft, sets lastCreatedId, and bumps version', async () => {
    mockGeneratePalette.mockResolvedValue({
      id: 7,
      name: 'Layer_MedianCut',
      colors: [
        [15, 56, 15],
        [155, 188, 15],
      ],
      hex_colors: ['#0f380f', '#9bbc0f'],
      color_count: 2,
    });

    const store = createTestStore({
      colorLab: {
        name: 'Untitled Palette',
        colors: [],
        extractMethod: 'MedianCut',
        extractCount: 8,
        error: null,
        successMessage: null,
        suppressRemote: false,
        remoteEpoch: 0,
      },
    });

    const result = await store.dispatch(extractPalette({ layerId: 1 }));
    expect(extractPalette.fulfilled.match(result)).toBe(true);
    expect(mockGeneratePalette).toHaveBeenCalledWith(1, 8, 'MedianCut');

    const state = store.getState();
    expect(state.palettes.lastCreatedId).toBe(7);
    expect(state.palettes.version).toBe(1);
    expect(state.colorLab.name).toBe('Layer_MedianCut');
    expect(state.colorLab.colors).toHaveLength(2);
    expect(state.colorLab.colors[0].hex).toBe('#0f380f');
    expect(state.colorLab.error).toBeNull();
  });

  it('extractPalette failures set Color Lab error without throwing past rejectValue', async () => {
    mockGeneratePalette.mockRejectedValue('No tile data');

    const store = createTestStore();
    const result = await store.dispatch(extractPalette({ layerId: 1 }));
    expect(extractPalette.rejected.match(result)).toBe(true);
    expect(store.getState().colorLab.error).toBe('No tile data');
    expect(store.getState().palettes.lastCreatedId).toBeNull();
  });

  it('maybeAutoExtractPalette skips generate_palette when toggle is off', async () => {
    const store = createTestStore();
    await maybeAutoExtractPalette(store.dispatch, 1, false);
    expect(mockGeneratePalette).not.toHaveBeenCalled();
  });

  it('maybeAutoExtractPalette runs extract when toggle is on (open/import path)', async () => {
    mockGeneratePalette.mockResolvedValue({
      id: 3,
      name: 'Imported_MedianCut',
      colors: [[0, 0, 0]],
      hex_colors: ['#000000'],
      color_count: 1,
    });

    const store = createTestStore();
    // Simulate existing filter with palette_id elsewhere — auto-extract only updates
    // palettesSlice / colorLab, never rewrites filter params in the store.
    store.dispatch(bumpVersion({ lastCreatedId: 99 }));
    const priorLast = store.getState().palettes.lastCreatedId;

    await maybeAutoExtractPalette(store.dispatch, 2, true);

    expect(mockGeneratePalette).toHaveBeenCalledWith(2, 8, 'MedianCut');
    expect(store.getState().palettes.lastCreatedId).toBe(3);
    // Prior lastCreatedId is replaced by the new extract; filter palette_ids live
    // in the engine document and are never patched by this frontend path.
    expect(priorLast).toBe(99);
    expect(store.getState().colorLab.name).toBe('Imported_MedianCut');
  });
});
