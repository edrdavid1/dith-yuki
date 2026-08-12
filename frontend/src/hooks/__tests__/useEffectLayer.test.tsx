import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useEffectLayer } from '../useEffectLayer';
import { createTestStore, StoreProvider } from '../../app/__tests__/testStore';
import type { FilterInfo } from '../../types';
import type { ReactNode } from 'react';

vi.mock('../../shared/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../shared/ipc')>();
  return {
    ...actual,
    updateFilter: vi.fn(),
  };
});

import { updateFilter } from '../../shared/ipc';

const mockUpdateFilter = vi.mocked(updateFilter);

function makeFilter(kind: string, params: Record<string, unknown>, id = 'filter-abc-123'): FilterInfo {
  return {
    id,
    kind: kind as FilterInfo['kind'],
    params: { type: kind, ...params } as FilterInfo['params'],
    enabled: true,
  };
}

function wrapperFor(filters: FilterInfo[]) {
  const store = createTestStore({
    filters: {
      byId: Object.fromEntries(filters.map((f) => [f.id, f])),
      orderOnImageSource: filters.map((f) => f.id),
      status: 'ready',
      error: null,
    },
  });
  return {
    store,
    wrapper: ({ children }: { children: ReactNode }) => (
      <StoreProvider store={store}>{children}</StoreProvider>
    ),
  };
}

describe('useEffectLayer', () => {
  beforeEach(() => {
    mockUpdateFilter.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns null state when layerId is null', () => {
    const { wrapper } = wrapperFor([]);
    const { result } = renderHook(() => useEffectLayer(null, null), { wrapper });

    expect(result.current.effectType).toBeNull();
    expect(result.current.effectParams).toBeNull();
    expect(result.current.filterId).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it('derives effect type from filters in the store', () => {
    const { wrapper } = wrapperFor([
      makeFilter('DitherV2', {
        mode: 'floyd_steinberg',
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: 'rgb',
        palette_id: null,
      }),
    ]);

    const { result } = renderHook(() => useEffectLayer(1, null), { wrapper });

    expect(result.current.effectType).toBe('Dithering');
    expect(result.current.filterId).toBe('filter-abc-123');
    expect(result.current.effectParams).toMatchObject({
      type: 'DitherV2',
      mode: 'floyd_steinberg',
      levels: 4,
    });
    expect(result.current.error).toBeNull();
  });

  it('derives Glitching effect type from Glitch filter kind', () => {
    const { wrapper } = wrapperFor([
      makeFilter('Glitch', {
        glitch_type: 'RGBShift',
        intensity: 0.5,
        seed: 42,
      }),
    ]);

    const { result } = renderHook(() => useEffectLayer(2, null), { wrapper });

    expect(result.current.effectType).toBe('Glitching');
    expect(result.current.effectParams).toMatchObject({
      type: 'Glitch',
      glitch_type: 'RGBShift',
      intensity: 0.5,
    });
  });

  it('returns null effect when store has no filters', () => {
    const { wrapper } = wrapperFor([]);
    const { result } = renderHook(() => useEffectLayer(5, null), { wrapper });

    expect(result.current.effectType).toBeNull();
    expect(result.current.effectParams).toBeNull();
    expect(result.current.filterId).toBeNull();
  });

  it('debounces updateParams calls by 100ms', async () => {
    vi.useFakeTimers();
    mockUpdateFilter.mockResolvedValue(undefined);

    const { wrapper } = wrapperFor([
      makeFilter('DitherV2', {
        mode: 'floyd_steinberg',
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: 'rgb',
        palette_id: null,
      }),
    ]);

    const { result } = renderHook(() => useEffectLayer(1, 'filter-abc-123'), { wrapper });

    expect(result.current.filterId).toBe('filter-abc-123');

    act(() => {
      result.current.updateParams({ levels: 8 });
    });

    expect(result.current.effectParams).toMatchObject({ levels: 8 });
    expect(mockUpdateFilter).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

    expect(mockUpdateFilter).toHaveBeenCalledWith(1, 'filter-abc-123', { levels: 8 });
  });

  it('rolls back optimistic params when updateFilter fails', async () => {
    vi.useFakeTimers();
    mockUpdateFilter.mockRejectedValue(new Error('boom'));

    const { wrapper } = wrapperFor([
      makeFilter('DitherV2', {
        mode: 'floyd_steinberg',
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: 'rgb',
        palette_id: null,
      }),
    ]);

    const { result } = renderHook(() => useEffectLayer(1, 'filter-abc-123'), { wrapper });

    act(() => {
      result.current.updateParams({ levels: 16 });
    });
    expect(result.current.effectParams).toMatchObject({ levels: 16 });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

    expect(result.current.effectParams).toMatchObject({ levels: 4 });
    expect(result.current.error).toBeTruthy();
  });
});
