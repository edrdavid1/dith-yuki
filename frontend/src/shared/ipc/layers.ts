import { invoke } from '@tauri-apps/api/core';
import type { LayerNodeDto, LayerPropsPatch } from '../../shared/types/layers';

/** VS Code / Photoshop style: every mutation targets an explicit document id. */
function withDocId(docId: number, body: Record<string, unknown>) {
  // Only snake_case: serde alias `docId` on the same field rejects duplicates.
  return { ...body, doc_id: docId };
}

export async function getLayerTree(): Promise<LayerNodeDto[]> {
  return invoke<LayerNodeDto[]>('get_layer_tree');
}

export async function addLayer(
  docId: number,
  kind: string,
  parentGroup: number | null,
  index: number
): Promise<{ layer_id: number }> {
  return invoke<{ layer_id: number }>('add_layer', {
    req: withDocId(docId, { kind, parent_group: parentGroup, index }),
  });
}

export async function removeLayer(docId: number, layerId: number): Promise<void> {
  return invoke<void>('remove_layer', { docId, layer_id: layerId });
}

export async function reorderLayer(
  docId: number,
  layerId: number,
  newParent: number | null,
  newIndex: number
): Promise<void> {
  return invoke<void>('reorder_layer', {
    req: withDocId(docId, {
      layer_id: layerId,
      new_parent: newParent,
      new_index: newIndex,
    }),
  });
}

export async function setLayerProps(
  docId: number,
  layerId: number,
  patch: LayerPropsPatch
): Promise<void> {
  return invoke<void>('set_layer_props', {
    req: withDocId(docId, {
      layer_id: layerId,
      name: patch.name ?? null,
      opacity: patch.opacity ?? null,
      blend_mode: patch.blend_mode ?? null,
      visible: patch.visible ?? null,
    }),
  });
}
