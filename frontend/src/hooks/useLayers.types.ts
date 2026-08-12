import type { LayerNodeDto, LayerPropsPatch } from '../shared/types/layers';
import type { EffectType } from '../types/effects';

export interface UseLayersReturn {
  layers: LayerNodeDto[];
  selectedLayerId: number | null;
  setSelectedLayerId: (id: number | null) => void;
  addLayer: () => void;
  removeLayer: (layerId: number) => void;
  addLayerWithEffect: (effectType: EffectType, position: number) => void;
  toggleVisibility: (layerId: number) => void;
  reorderLayer: (layerId: number, newParent: number | null, newIndex: number) => void;
  setLayerProps: (layerId: number, patch: LayerPropsPatch) => void;
  refreshLayers: () => Promise<void>;
  error: string | null;
}
