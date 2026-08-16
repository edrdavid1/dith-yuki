import { useCallback, useEffect, useMemo, useRef } from 'react';
import EffectSettingsPanel from './EffectSettingsPanel';
import type { LayerWithFilters } from './EffectSettingsPanel';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { addLayerWithEffect } from '../../app/slices/layersSlice';
import { refreshFilters, selectFiltersList } from '../../app/slices/filtersSlice';
import { setSelection } from '../../app/slices/selectionSlice';
import { useEffectLayer } from '../../hooks/useEffectLayer';
import { useDocument } from '../../hooks/useDocument';
import type { EffectType } from '../../types/effects';
import type { FilterInfo } from '../../types';
import type { PanelChromeProps } from '../panels/PanelChrome';

function findLayerById(
  layers: { id: number; name: string; children?: unknown[] }[],
  id: number
): { id: number; name: string } | null {
  for (const layer of layers) {
    if (layer.id === id) return layer;
    if (layer.children) {
      const found = findLayerById(layer.children as typeof layers, id);
      if (found) return found;
    }
  }
  return null;
}

/**
 * Connected Effects feature — owns selection→params wiring and add-effect thunk.
 * Dither palette is bound to Color Lab (`palettes.lastCreatedId`), not a local picker.
 */
export default function EffectsFeature({
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
}: PanelChromeProps) {
  const dispatch = useAppDispatch();
  const layers = useAppSelector((s) => s.layers.tree);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const selectedFilterId = useAppSelector((s) => s.selection.filterId);
  const allFilters = useAppSelector(selectFiltersList);
  const lastCreatedId = useAppSelector((s) => s.palettes.lastCreatedId);
  const docId = useAppSelector((s) => s.document.docId);
  const doc = useDocument();

  const imageSourceLayer = layers.length > 0 ? layers[0] : null;
  const selectedFilter =
    selectedFilterId !== null
      ? allFilters.find((filter) => filter.id === selectedFilterId) ?? null
      : null;
  const currentLayerForEffect = selectedFilter
    ? imageSourceLayer?.id ?? selectedLayerId
    : selectedLayerId;

  const effectLayer = useEffectLayer(currentLayerForEffect, selectedFilterId);

  const selectedLayerWithFilters: LayerWithFilters | null = useMemo(() => {
    if (selectedFilterId === null) return null;
    if (!effectLayer.effectType || !effectLayer.effectParams || !effectLayer.filterId) {
      return null;
    }
    const layerId = currentLayerForEffect ?? selectedLayerId;
    if (layerId === null) return null;
    const layer = findLayerById(layers, layerId);
    if (!layer) return null;
    return {
      id: layer.id,
      name: layer.name,
      filters: [
        {
          id: effectLayer.filterId,
          kind: effectLayer.effectParams.type as FilterInfo['kind'],
          params: effectLayer.effectParams,
          enabled: selectedFilter?.enabled ?? true,
          opacity: effectLayer.opacity,
          blend_mode: effectLayer.blendMode,
        },
      ],
    };
  }, [
    selectedFilterId,
    effectLayer.effectType,
    effectLayer.effectParams,
    effectLayer.filterId,
    effectLayer.opacity,
    effectLayer.blendMode,
    selectedFilter?.enabled,
    currentLayerForEffect,
    selectedLayerId,
    layers,
  ]);

  const handleUpdateParams = useCallback(
    (_layerId: number, _filterId: string, params: Record<string, unknown>) => {
      effectLayer.updateParams(params);
    },
    [effectLayer.updateParams]
  );

  // Keep selected Dither filter's palette_id in sync with Color Lab.
  // Send only DitherV2 fields (never the UI `type` tag) so serde accepts the payload.
  // Do not write `palette_id: null` just because this window's store has not
  // received lastCreatedId yet (floating Color Lab vs Effects).
  const lastSyncedKeyRef = useRef<string>('');
  useEffect(() => {
    if (effectLayer.effectType !== 'Dithering') return;
    if (effectLayer.effectParams == null || effectLayer.filterId == null) return;
    if (lastCreatedId == null) return;
    const syncKey = `${effectLayer.filterId}:${lastCreatedId}`;
    if (lastSyncedKeyRef.current === syncKey) return;

    const params = effectLayer.effectParams as unknown as Record<string, unknown>;
    const current =
      typeof params.palette_id === 'number' ? params.palette_id : null;
    if (current === lastCreatedId) {
      lastSyncedKeyRef.current = syncKey;
      return;
    }
    lastSyncedKeyRef.current = syncKey;

    const {
      mode,
      levels,
      threshold_scale,
      pixel_size,
      color_mode,
      halftone_cell_size,
      wave_wavelength,
      wave_amplitude,
      wave_phase,
      wave_angle,
      threshold_bias,
      pattern_angle,
      serpentine,
      dither_alpha,
    } = params;
    effectLayer.updateParams({
      mode,
      levels,
      threshold_scale,
      pixel_size,
      color_mode: color_mode ?? 'rgb',
      palette_id: lastCreatedId,
      halftone_cell_size: halftone_cell_size ?? 8,
      wave_wavelength: wave_wavelength ?? 8,
      wave_amplitude: wave_amplitude ?? 1,
      wave_phase: wave_phase ?? 0,
      wave_angle: wave_angle ?? 0,
      threshold_bias: threshold_bias ?? 0,
      pattern_angle: pattern_angle ?? 0,
      serpentine: serpentine ?? false,
      dither_alpha: dither_alpha !== false,
    });
  }, [
    lastCreatedId,
    effectLayer.effectType,
    effectLayer.effectParams,
    effectLayer.filterId,
    effectLayer.updateParams,
  ]);

  const handleSelectEffect = useCallback(
    (effectType: EffectType) => {
      void dispatch(addLayerWithEffect({ docId, layers, effectType })).then((result) => {
        if (addLayerWithEffect.fulfilled.match(result) && result.payload != null) {
          void dispatch(setSelection({ layerId: result.payload, filterId: null }));
          void dispatch(refreshFilters());
        }
      });
    },
    [dispatch, docId, layers]
  );

  return (
    <EffectSettingsPanel
      selectedLayer={selectedLayerWithFilters}
      onUpdateParams={handleUpdateParams}
      onSelectEffect={handleSelectEffect}
      onTitleBarMouseDown={onTitleBarMouseDown}
      dockSide={dockSide}
      onMoveToSide={onMoveToSide}
      targetLayerId={currentLayerForEffect ?? selectedLayerId}
      onExportPattern={() =>
        void doc.exportPattern(currentLayerForEffect ?? selectedLayerId)
      }
      onImportPattern={() =>
        void doc.importPattern(currentLayerForEffect ?? selectedLayerId)
      }
    />
  );
}
