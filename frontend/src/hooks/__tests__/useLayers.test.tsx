import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useLayers } from '../useLayers';
import { StoreProvider, createTestStore } from '../../app/__tests__/testStore';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
  emitTo: vi.fn(),
}));

vi.mock('../../shared/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../shared/ipc')>();
  return {
    ...actual,
    addFilter: vi.fn(),
    addLayer: vi.fn(),
  };
});

import { invoke } from '@tauri-apps/api/core';
import { addFilter, addLayer as addLayerIPC } from '../../shared/ipc';

const mockInvoke = vi.mocked(invoke);
const mockAddLayer = vi.mocked(addLayerIPC);
const mockAddFilter = vi.mocked(addFilter);

function makeLayers() {
  return [
    { id: 1, name: 'Layer 1', kind: 'raster', blend_mode: 'Normal', opacity: 1.0, visible: true },
    { id: 2, name: 'Image Source', kind: 'raster', blend_mode: 'Normal', opacity: 1.0, visible: true },
  ];
}

function makeValidSnapshot() {
  return {
    snapshot: {
      layers: [
        { id: { inner: 1 }, filters: [{ id: 'f1', kind: 'DitherV2', params: {}, enabled: true }] },
        { id: { inner: 2 }, filters: [] },
      ],
    },
  };
}

function makeInvalidSnapshot() {
  return {
    snapshot: {
      layers: [
        { id: { inner: 2 }, filters: [] },
        {
          id: { inner: 1 },
          filters: [
            { id: 'f1', kind: 'DitherV2', params: {}, enabled: true },
            { id: 'f2', kind: 'Glitch', params: {}, enabled: true },
          ],
        },
      ],
    },
  };
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <StoreProvider store={createTestStore()}>{children}</StoreProvider>;
}

describe('useLayers', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockAddLayer.mockReset();
    mockAddFilter.mockReset();
  });

  describe('initial fetch and validation', () => {
    it('fetches layer tree on mount when docId is present', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });
      expect(result.current.error).toBeNull();
    });

    it('returns empty layers when docId is null', async () => {
      const { result } = renderHook(() => useLayers({ docId: null }), { wrapper });
      await waitFor(() => {
        expect(result.current.layers).toEqual([]);
      });
      expect(result.current.error).toBeNull();
    });

    it('sets error when document validation fails', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeInvalidSnapshot();
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.error).toContain('Invalid document structure');
        expect(result.current.error).toContain('layer 1');
      });
      expect(result.current.layers).toHaveLength(2);
    });
  });

  describe('removeLayer', () => {
    it('calls remove_layer IPC and refreshes', async () => {
      const layers = makeLayers();
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return layers;
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        if (cmd === 'remove_layer') return undefined;
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.removeLayer(1);
      });

      expect(mockInvoke).toHaveBeenCalledWith('remove_layer', { layer_id: 1 });
    });

    it('sets error on IPC failure', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        if (cmd === 'remove_layer') throw 'Failed to remove layer';
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.removeLayer(1);
      });

      await waitFor(() => {
        expect(result.current.error).toBe('Failed to remove layer');
      });
    });
  });

  describe('addLayerWithEffect', () => {
    it('creates layer then adds filter with correct params', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });
      mockAddFilter.mockResolvedValue({ filter_id: 'new-filter-1' });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.addLayerWithEffect('Dithering', 1);
      });

      expect(mockAddLayer).not.toHaveBeenCalled();
      expect(mockAddFilter).toHaveBeenCalledWith(1, 'DitherV2', {
        mode: 'floyd_steinberg',
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: 'rgb',
        palette_id: null,
      });

      await waitFor(() => {
        expect(result.current.selectedLayerId).toBe(1);
      });
    });

    it('defaults palette_id to lastCreatedId when set', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });
      mockAddFilter.mockResolvedValue({ filter_id: 'new-filter-palette' });

      const store = createTestStore({
        palettes: { version: 1, lastCreatedId: 42, error: null },
      });
      const localWrapper = ({ children }: { children: React.ReactNode }) => (
        <StoreProvider store={store}>{children}</StoreProvider>
      );

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper: localWrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.addLayerWithEffect('Dithering', 1);
      });

      expect(mockAddFilter).toHaveBeenCalledWith(
        1,
        'DitherV2',
        expect.objectContaining({ palette_id: 42 })
      );
    });

    it('calls addFilter with Glitch kind for Glitching effect', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });
      mockAddFilter.mockResolvedValue({ filter_id: 'new-filter-2' });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.addLayerWithEffect('Glitching', 2);
      });

      expect(mockAddLayer).not.toHaveBeenCalled();
      expect(mockAddFilter).toHaveBeenCalledWith(1, 'Glitch', {
        glitch_type: 'RGBShift',
        intensity: 0.5,
        seed: 0,
      });
    });

    it('sets error on addFilter failure', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });
      mockAddFilter.mockRejectedValue('Failed to add filter');

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.addLayerWithEffect('Curves', 0);
      });

      await waitFor(() => {
        expect(result.current.error).toBe('Failed to add filter');
      });
    });
  });

  describe('toggleVisibility', () => {
    it('flips visible from true to false', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        if (cmd === 'set_layer_props') return undefined;
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      await act(async () => {
        await result.current.toggleVisibility(1);
      });

      expect(mockInvoke).toHaveBeenCalledWith('set_layer_props', {
        req: {
          layer_id: 1,
          name: null,
          opacity: null,
          blend_mode: null,
          visible: false,
        },
      });
    });

    it('does nothing if layer not found', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_layer_tree') return makeLayers();
        if (cmd === 'get_document_snapshot') return makeValidSnapshot();
        return undefined;
      });

      const { result } = renderHook(() => useLayers({ docId: 1 }), { wrapper });

      await waitFor(() => {
        expect(result.current.layers).toHaveLength(2);
      });

      const callCountBefore = mockInvoke.mock.calls.filter((c) => c[0] === 'set_layer_props').length;

      await act(async () => {
        await result.current.toggleVisibility(999);
      });

      const callCountAfter = mockInvoke.mock.calls.filter((c) => c[0] === 'set_layer_props').length;
      expect(callCountAfter).toBe(callCountBefore);
    });
  });

  describe('no-op when docId is null', () => {
    it('removeLayer is a no-op when docId is null', async () => {
      const { result } = renderHook(() => useLayers({ docId: null }), { wrapper });

      await act(async () => {
        await result.current.removeLayer(1);
      });

      expect(mockInvoke).not.toHaveBeenCalledWith('remove_layer', expect.anything());
    });

    it('addLayerWithEffect is a no-op when docId is null', async () => {
      const { result } = renderHook(() => useLayers({ docId: null }), { wrapper });

      await act(async () => {
        await result.current.addLayerWithEffect('Dithering', 0);
      });

      expect(mockAddLayer).not.toHaveBeenCalled();
    });

    it('toggleVisibility is a no-op when docId is null', async () => {
      const { result } = renderHook(() => useLayers({ docId: null }), { wrapper });

      await act(async () => {
        await result.current.toggleVisibility(1);
      });

      expect(mockInvoke).not.toHaveBeenCalledWith('set_layer_props', expect.anything());
    });
  });
});
