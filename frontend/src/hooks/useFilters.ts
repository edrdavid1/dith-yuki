import { useState, useCallback, useRef } from 'react';
import { addFilter as addFilterIPC, updateFilter as updateFilterIPC, removeFilter as removeFilterIPC } from '../ipc/commands';
import type { FilterInfo, FilterKind } from '../types';

// Default params for each filter kind.
// Defaults should produce a VISIBLE change so the user gets immediate feedback.
function getDefaultParams(kind: FilterKind): Record<string, unknown> {
  switch (kind) {
    case 'Dither':
      // color_depth: 1 = 1-bit (black/white) dithering — very obvious
      return { algorithm: 'FloydSteinberg', color_depth: 1 };
    case 'Curves':
      // Identity curve — user will adjust control points interactively
      return { curve: [[0, 0], [1, 1]], channel: 'All' };
    case 'Levels':
      // gamma: 2.0 produces noticeable brightening of midtones
      return { input_black: 0.0, input_white: 1.0, gamma: 2.0, output_black: 0.0, output_white: 1.0 };
    case 'Glitch':
      return { glitch_type: 'RGBShift', intensity: 0.5, seed: Math.floor(Math.random() * 100000) };
  }
}

export function useFilters(layerId: number | null, onRefresh: () => void) {
  const [filters, setFilters] = useState<FilterInfo[]>([]);
  const [activeFilterId, setActiveFilterId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>();
  const filtersRef = useRef<FilterInfo[]>([]);

  // Keep filtersRef in sync for use inside debounced closures
  filtersRef.current = filters;

  const addFilter = useCallback(async (kind: FilterKind) => {
    if (layerId === null) return;
    setError(null);

    try {
      const params = getDefaultParams(kind);
      const { filter_id } = await addFilterIPC(layerId, kind, params);

      const newFilter: FilterInfo = {
        id: filter_id,
        kind,
        params: { type: kind, ...params } as FilterInfo['params'],
        enabled: true,
      };

      setFilters(prev => [...prev, newFilter]);
      setActiveFilterId(filter_id);
      onRefresh();
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    }
  }, [layerId, onRefresh]);

  const updateFilterParams = useCallback(async (filterId: string, params: Record<string, unknown>) => {
    if (layerId === null) return;
    setError(null);

    // Debounce 100ms
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      const prevFilters = [...filtersRef.current];
      try {
        await updateFilterIPC(layerId, filterId, params);
        setFilters(prev => prev.map(f =>
          f.id === filterId ? { ...f, params: { ...f.params, ...params } as FilterInfo['params'] } : f
        ));
        onRefresh();
      } catch (err) {
        setError(typeof err === 'string' ? err : String(err));
        setFilters(prevFilters); // Rollback
      }
    }, 100);
  }, [layerId, onRefresh]);

  const removeFilter = useCallback(async (filterId: string) => {
    if (layerId === null) return;
    setError(null);

    const prevFilters = [...filtersRef.current];
    try {
      await removeFilterIPC(layerId, filterId);
      setFilters(prev => prev.filter(f => f.id !== filterId));
      if (activeFilterId === filterId) {
        setActiveFilterId(null);
      }
      onRefresh();
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
      setFilters(prevFilters); // Rollback
    }
  }, [layerId, activeFilterId, onRefresh]);

  return {
    filters,
    activeFilterId,
    activeFilter: filters.find(f => f.id === activeFilterId) ?? null,
    error,
    addFilter,
    updateFilterParams,
    removeFilter,
    setActiveFilterId,
  };
}
