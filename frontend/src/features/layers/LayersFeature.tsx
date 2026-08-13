import { useCallback } from 'react';
import LayersPanel from '../../components/LayersPanel';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import {
  removeFilter as removeFilterThunk,
  reorderFilter as reorderFilterThunk,
  selectFiltersList,
} from '../../app/slices/filtersSlice';
import {
  patchLayerProps,
  refreshLayers,
  toggleLayerVisibility,
} from '../../app/slices/layersSlice';
import { setSelection } from '../../app/slices/selectionSlice';
import { useEffectLayer } from '../../hooks/useEffectLayer';
import { logIpcError } from '../../shared/ipc';
import type { PanelChromeProps } from '../panels/PanelChrome';

/**
 * Connected Layers feature — reads/writes RTK; layout only passes chrome props.
 */
export default function LayersFeature({
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
}: PanelChromeProps) {
  const dispatch = useAppDispatch();
  const layers = useAppSelector((s) => s.layers.tree);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const selectedFilterId = useAppSelector((s) => s.selection.filterId);
  const filters = useAppSelector(selectFiltersList);
  const docId = useAppSelector((s) => s.document.docId);

  const imageSourceLayer = layers.length > 0 ? layers[0] : null;
  const effectLayer = useEffectLayer(imageSourceLayer?.id ?? null, selectedFilterId);

  const handleSelect = useCallback(
    (layerId: number) => {
      if (layerId === selectedLayerId) {
        void dispatch(setSelection({ layerId: null, filterId: null }));
        return;
      }
      void dispatch(setSelection({ layerId, filterId: null }));
    },
    [dispatch, selectedLayerId]
  );

  const handleSelectFilter = useCallback(
    (filterId: string | null) => {
      if (filterId === selectedFilterId) {
        void dispatch(setSelection({ layerId: null, filterId: null }));
        return;
      }
      if (imageSourceLayer) {
        void dispatch(setSelection({ layerId: imageSourceLayer.id, filterId }));
      } else {
        void dispatch(setSelection({ layerId: null, filterId }));
      }
    },
    [dispatch, imageSourceLayer, selectedFilterId]
  );

  const handleAddLayer = useCallback(() => {
    void dispatch(setSelection({ layerId: null, filterId: null }));
  }, [dispatch]);

  const handleRemoveFilter = useCallback(
    async (filterId: string) => {
      const targetLayerId = imageSourceLayer?.id ?? selectedLayerId;
      if (targetLayerId === null) return;
      try {
        await dispatch(removeFilterThunk({ layerId: targetLayerId, filterId })).unwrap();
        void dispatch(setSelection({ layerId: targetLayerId, filterId: null }));
        await dispatch(refreshLayers(docId));
      } catch (err) {
        logIpcError('LayersFeature.removeFilter', err);
      }
    },
    [dispatch, docId, imageSourceLayer, selectedLayerId]
  );

  const handleReorderFilter = useCallback(
    async (filterId: string, newIndex: number) => {
      const targetLayerId = imageSourceLayer?.id;
      if (targetLayerId == null) return;
      try {
        await dispatch(
          reorderFilterThunk({ layerId: targetLayerId, filterId, newIndex })
        ).unwrap();
        await dispatch(refreshLayers(docId));
      } catch (err) {
        logIpcError('LayersFeature.reorderFilter', err);
      }
    },
    [dispatch, docId, imageSourceLayer]
  );

  const handleToggleVisibility = useCallback(
    (layerId: number) => {
      void dispatch(toggleLayerVisibility({ docId, layerId, layers }));
    },
    [dispatch, docId, layers]
  );

  const handleBlendModeChange = useCallback(
    (layerId: number, mode: string) => {
      void dispatch(patchLayerProps({ docId, layerId, patch: { blend_mode: mode } }));
    },
    [dispatch, docId]
  );

  const handleOpacityChange = useCallback(
    (layerId: number, opacity: number) => {
      void dispatch(patchLayerProps({ docId, layerId, patch: { opacity } }));
    },
    [dispatch, docId]
  );

  const handleFilterBlendChange = useCallback(
    (patch: { opacity?: number; blend_mode?: string }) => {
      effectLayer.updateBlend(patch);
    },
    [effectLayer.updateBlend]
  );

  return (
    <LayersPanel
      layers={layers}
      selectedLayerId={selectedLayerId}
      filters={filters}
      selectedFilterId={selectedFilterId}
      onSelect={handleSelect}
      onSelectFilter={handleSelectFilter}
      onAddLayer={handleAddLayer}
      onRemoveFilter={handleRemoveFilter}
      onReorderFilter={handleReorderFilter}
      onToggleVisibility={handleToggleVisibility}
      onBlendModeChange={handleBlendModeChange}
      onOpacityChange={handleOpacityChange}
      onFilterBlendChange={handleFilterBlendChange}
      onTitleBarMouseDown={onTitleBarMouseDown}
      dockSide={dockSide}
      onMoveToSide={onMoveToSide}
    />
  );
}
