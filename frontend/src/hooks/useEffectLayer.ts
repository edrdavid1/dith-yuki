import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { useAppSelector } from '../app/hooks';
import { selectFiltersList } from '../app/slices/filtersSlice';
import { formatIpcError, logIpcError, updateFilter } from '../shared/ipc';
import type { FilterParams } from '../types';
import { EFFECT_TO_FILTER_KIND } from '../types/effects';
import type { EffectType } from '../types/effects';

export interface UseEffectLayerReturn {
  effectType: EffectType | null;
  effectParams: FilterParams | null;
  filterId: string | null;
  updateParams: (params: Record<string, unknown>) => void;
  error: string | null;
}

function filterKindToEffectType(kind: string): EffectType | null {
  for (const [effectType, filterKind] of Object.entries(EFFECT_TO_FILTER_KIND)) {
    if (filterKind === kind) {
      return effectType as EffectType;
    }
  }
  if (kind === 'Dither') {
    return 'Dithering';
  }
  return null;
}

function toFilterParams(kind: string, params: Record<string, unknown>): FilterParams {
  if (params.DitherV2 && typeof params.DitherV2 === 'object') {
    return { type: kind, ...(params.DitherV2 as Record<string, unknown>) } as unknown as FilterParams;
  }
  if (params.Glow && typeof params.Glow === 'object') {
    return { type: kind, ...(params.Glow as Record<string, unknown>) } as unknown as FilterParams;
  }
  if (params.Crt && typeof params.Crt === 'object') {
    return { type: kind, ...(params.Crt as Record<string, unknown>) } as unknown as FilterParams;
  }
  return { type: kind, ...params } as unknown as FilterParams;
}

const DEBOUNCE_MS = 100;

/**
 * Effect editor state for the selected filter.
 * Reads filters from RTK (hydrated by engine event bridge) — no local `listen`.
 * `updateParams` stays debounced with optimistic update + rollback.
 */
export function useEffectLayer(
  layerId: number | null,
  selectedFilterId: string | null
): UseEffectLayerReturn {
  const filters = useAppSelector(selectFiltersList);
  const [optimisticParams, setOptimisticParams] = useState<FilterParams | null>(null);
  const [error, setError] = useState<string | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout>>();
  const layerIdRef = useRef<number | null>(layerId);
  const paramsRef = useRef<FilterParams | null>(null);
  const filterIdRef = useRef<string | null>(null);

  layerIdRef.current = layerId;

  const storeFilter = useMemo(() => {
    if (layerId === null) return null;
    if (selectedFilterId !== null) {
      return filters.find((f) => f.id === selectedFilterId) ?? null;
    }
    return filters[0] ?? null;
  }, [filters, layerId, selectedFilterId]);

  const storeKey = storeFilter
    ? `${storeFilter.id}:${JSON.stringify(storeFilter.params)}`
    : '';

  const storeParams = useMemo(() => {
    if (!storeFilter) return null;
    return toFilterParams(storeFilter.kind, storeFilter.params as unknown as Record<string, unknown>);
  }, [storeFilter]);

  const effectType = storeFilter ? filterKindToEffectType(storeFilter.kind) : null;
  const filterId = storeFilter?.id ?? null;
  const effectParams = optimisticParams ?? storeParams;

  paramsRef.current = effectParams;
  filterIdRef.current = filterId;

  // Drop optimistic overlay when the engine mirror replaces this filter
  useEffect(() => {
    setOptimisticParams(null);
    setError(null);
  }, [storeKey]);

  const updateParams = useCallback((params: Record<string, unknown>) => {
    if (layerIdRef.current === null || filterIdRef.current === null) return;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    const prevParams = paramsRef.current;
    setOptimisticParams((current) => {
      const base = current ?? prevParams;
      if (base === null) return null;
      return { ...base, ...params } as FilterParams;
    });
    setError(null);

    debounceRef.current = setTimeout(async () => {
      try {
        await updateFilter(layerIdRef.current!, filterIdRef.current!, params);
      } catch (err) {
        logIpcError('useEffectLayer.updateFilter', err);
        setOptimisticParams(prevParams);
        setError(formatIpcError(err));
      }
    }, DEBOUNCE_MS);
  }, []);

  useEffect(() => {
    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, []);

  return {
    effectType,
    effectParams,
    filterId,
    updateParams,
    error,
  };
}
