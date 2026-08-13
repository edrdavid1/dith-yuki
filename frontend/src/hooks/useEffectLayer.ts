import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { patchFilter, refreshFilters, selectFiltersList } from '../app/slices/filtersSlice';
import { formatIpcError, logIpcError, updateFilter } from '../shared/ipc';
import type { FilterParams } from '../types';
import { EFFECT_TO_FILTER_KIND } from '../types/effects';
import type { EffectType } from '../types/effects';
import { unwrapFilterParams } from '../shared/unwrapFilterParams';

export interface UseEffectLayerReturn {
  effectType: EffectType | null;
  effectParams: FilterParams | null;
  filterId: string | null;
  opacity: number;
  blendMode: string;
  updateParams: (params: Record<string, unknown>) => void;
  updateBlend: (patch: { opacity?: number; blend_mode?: string }) => void;
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
  return { type: kind, ...unwrapFilterParams(params) } as unknown as FilterParams;
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
  const dispatch = useAppDispatch();
  const filters = useAppSelector(selectFiltersList);
  const [optimisticParams, setOptimisticParams] = useState<FilterParams | null>(null);
  const [optimisticOpacity, setOptimisticOpacity] = useState<number | null>(null);
  const [optimisticBlend, setOptimisticBlend] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout>>();
  const blendDebounceRef = useRef<ReturnType<typeof setTimeout>>();
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
    ? `${storeFilter.id}:${JSON.stringify(storeFilter.params)}:${storeFilter.opacity}:${storeFilter.blend_mode}`
    : '';

  const storeParams = useMemo(() => {
    if (!storeFilter) return null;
    return toFilterParams(storeFilter.kind, storeFilter.params as unknown as Record<string, unknown>);
  }, [storeFilter]);

  const effectType = storeFilter ? filterKindToEffectType(storeFilter.kind) : null;
  const filterId = storeFilter?.id ?? null;
  const effectParams = optimisticParams ?? storeParams;
  const opacity = optimisticOpacity ?? storeFilter?.opacity ?? 1;
  const blendMode = optimisticBlend ?? storeFilter?.blend_mode ?? 'Normal';

  paramsRef.current = effectParams;
  filterIdRef.current = filterId;

  // Drop optimistic overlay when the engine mirror replaces this filter
  useEffect(() => {
    setOptimisticParams(null);
    setOptimisticOpacity(null);
    setOptimisticBlend(null);
    setError(null);
  }, [storeKey]);

  const updateBlend = useCallback((patch: { opacity?: number; blend_mode?: string }) => {
    if (layerIdRef.current === null || filterIdRef.current === null) return;
    if (paramsRef.current === null) return;

    if (blendDebounceRef.current) {
      clearTimeout(blendDebounceRef.current);
    }

    if (typeof patch.opacity === 'number') {
      setOptimisticOpacity(patch.opacity);
    }
    if (typeof patch.blend_mode === 'string') {
      setOptimisticBlend(patch.blend_mode);
    }
    dispatch(patchFilter({ id: filterIdRef.current, ...patch }));
    setError(null);

    const currentParams = paramsRef.current as unknown as Record<string, unknown>;
    const { type: _type, ...params } = currentParams;

    blendDebounceRef.current = setTimeout(async () => {
      try {
        await updateFilter(layerIdRef.current!, filterIdRef.current!, params, patch);
      } catch (err) {
        logIpcError('useEffectLayer.updateFilterBlend', err);
        setOptimisticOpacity(null);
        setOptimisticBlend(null);
        void dispatch(refreshFilters());
        setError(formatIpcError(err));
      }
    }, DEBOUNCE_MS);
  }, [dispatch]);

  const updateParams = useCallback((params: Record<string, unknown>) => {
    if (layerIdRef.current === null || filterIdRef.current === null) return;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    const prevParams = paramsRef.current;
    setOptimisticParams((current) => {
      const base = current ?? prevParams;
      if (base === null) return null;
      const record = base as unknown as Record<string, unknown>;
      const kind = typeof record.type === 'string' ? record.type : 'Curves';
      return { type: kind, ...unwrapFilterParams(record), ...params } as FilterParams;
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
      if (blendDebounceRef.current) {
        clearTimeout(blendDebounceRef.current);
      }
    };
  }, []);

  return {
    effectType,
    effectParams,
    filterId,
    opacity,
    blendMode,
    updateParams,
    updateBlend,
    error,
  };
}
