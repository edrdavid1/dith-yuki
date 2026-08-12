import { invoke } from '@tauri-apps/api/core';
import type { LayerNodeDto, LayerPropsPatch } from '../../shared/types/layers';

export async function getLayerTree(): Promise<LayerNodeDto[]> {
  return invoke<LayerNodeDto[]>('get_layer_tree');
}

export async function addLayer(
  kind: string,
  parentGroup: number | null,
  index: number
): Promise<{ layer_id: number }> {
  return invoke<{ layer_id: number }>('add_layer', {
    req: { kind, parent_group: parentGroup, index },
  });
}

export async function removeLayer(layerId: number): Promise<void> {
  return invoke<void>('remove_layer', { layer_id: layerId });
}

export async function reorderLayer(
  layerId: number,
  newParent: number | null,
  newIndex: number
): Promise<void> {
  return invoke<void>('reorder_layer', {
    req: {
      layer_id: layerId,
      new_parent: newParent,
      new_index: newIndex,
    },
  });
}

export async function setLayerProps(
  layerId: number,
  patch: LayerPropsPatch
): Promise<void> {
  return invoke<void>('set_layer_props', {
    req: {
      layer_id: layerId,
      name: patch.name ?? null,
      opacity: patch.opacity ?? null,
      blend_mode: patch.blend_mode ?? null,
      visible: patch.visible ?? null,
    },
  });
}
