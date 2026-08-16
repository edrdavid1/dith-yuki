import { describe, it, expect, beforeEach, vi } from 'vitest';
import { importImageLayer } from '../documentSlice';
import { maybeAutoExtractPalette } from '../../autoExtract';
import { createTestStore } from '../../__tests__/testStore';

vi.mock('../../../shared/ipc', async () => {
  const actual = await vi.importActual<typeof import('../../../shared/ipc')>(
    '../../../shared/ipc'
  );
  return {
    ...actual,
    importImageLayer: vi.fn(),
    generatePalette: vi.fn(),
    logIpcError: vi.fn(),
  };
});

import { importImageLayer as importImageLayerIPC, generatePalette } from '../../../shared/ipc';

const mockImport = vi.mocked(importImageLayerIPC);
const mockGeneratePalette = vi.mocked(generatePalette);

describe('importImageLayer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('stores the new layer id without replacing document size', async () => {
    mockImport.mockResolvedValue({ layer_id: 2 });
    const store = createTestStore({
      document: {
        docId: 1,
        width: 512,
        height: 512,
        hasDocument: true,
        loading: false,
        notification: null,
        error: null,
        layerId: 1,
        projectPath: '/tmp/qa.dyproj',
        dirty: false,
      },
    });

    const result = await store.dispatch(importImageLayer('/tmp/icon.png'));
    expect(importImageLayer.fulfilled.match(result)).toBe(true);
    const state = store.getState().document;
    expect(state.layerId).toBe(2);
    expect(state.width).toBe(512);
    expect(state.height).toBe(512);
    expect(state.docId).toBe(1);
    expect(state.hasDocument).toBe(true);
    expect(state.projectPath).toBe('/tmp/qa.dyproj');
  });

  it('does not extract a palette when the pref is off', async () => {
    const store = createTestStore();
    await maybeAutoExtractPalette(store.dispatch, 2, false);
    expect(mockGeneratePalette).not.toHaveBeenCalled();
  });
});
