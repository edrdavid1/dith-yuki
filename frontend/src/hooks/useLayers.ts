import { useCallback, useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import {
  addLayerWithEffect as addLayerWithEffectThunk,
  addRasterLayer,
  patchLayerProps,
  refreshLayers,
  removeLayer as removeLayerThunk,
  reorderLayer as reorderLayerThunk,
  toggleLayerVisibility,
} from '../app/slices/layersSlice';
import { setSelection } from '../app/slices/selectionSlice';
import { refreshFilters } from '../app/slices/filtersSlice';
import type { LayerPropsPatch } from '../shared/types/layers';
import type { EffectType } from '../types/effects';
import type { UseLayersReturn } from './useLayers.types';

export type { UseLayersReturn } from './useLayers.types';

interface UseLayersOptions {
  /** The current document ID; when null, layers are empty. */
  docId: number | null;
}

/**
 * Thin adapter over RTK `layers` + `selection` slices.
 * Keeps the previous hook API for App / tests during P1 migration.
 */
export function useLayers({ docId }: UseLayersOptions): UseLayersReturn {
  const dispatch = useAppDispatch();
  const layers = useAppSelector((s) => s.layers.tree);
  const error = useAppSelector((s) => s.layers.error);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);

  useEffect(() => {
    void dispatch(refreshLayers(docId));
    if (docId !== null) {
      void dispatch(refreshFilters());
    }
  }, [dispatch, docId]);

  const setSelectedLayerId = useCallback(
    (id: number | null) => {
      void dispatch(setSelection({ layerId: id, filterId: null }));
    },
    [dispatch]
  );

  const addLayer = useCallback(() => {
    void dispatch(addRasterLayer({ docId, selectedLayerId, layers }));
  }, [dispatch, docId, selectedLayerId, layers]);

  const removeLayer = useCallback(
    (layerId: number) => {
      void dispatch(removeLayerThunk({ docId, layerId }));
    },
    [dispatch, docId]
  );

  const addLayerWithEffect = useCallback(
    (effectType: EffectType, _position: number) => {
      void dispatch(addLayerWithEffectThunk({ docId, layers, effectType })).then((result) => {
        if (addLayerWithEffectThunk.fulfilled.match(result) && result.payload != null) {
          void dispatch(setSelection({ layerId: result.payload, filterId: null }));
          void dispatch(refreshFilters());
        }
      });
    },
    [dispatch, docId, layers]
  );

  const toggleVisibility = useCallback(
    (layerId: number) => {
      void dispatch(toggleLayerVisibility({ docId, layerId, layers }));
    },
    [dispatch, docId, layers]
  );

  const reorderLayer = useCallback(
    (layerId: number, newParent: number | null, newIndex: number) => {
      void dispatch(reorderLayerThunk({ docId, layerId, newParent, newIndex }));
    },
    [dispatch, docId]
  );

  const setLayerProps = useCallback(
    (layerId: number, patch: LayerPropsPatch) => {
      void dispatch(patchLayerProps({ docId, layerId, patch }));
    },
    [dispatch, docId]
  );

  const refreshLayersFn = useCallback(async () => {
    await dispatch(refreshLayers(docId));
    if (docId !== null) {
      await dispatch(refreshFilters());
    }
  }, [dispatch, docId]);

  return {
    layers,
    selectedLayerId,
    setSelectedLayerId,
    addLayer,
    removeLayer,
    addLayerWithEffect,
    toggleVisibility,
    reorderLayer,
    setLayerProps,
    refreshLayers: refreshLayersFn,
    error,
  };
}
