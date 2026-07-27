import { invoke } from '@tauri-apps/api/core';
import type { LoadImageResponse, ExportImageRequest } from '../types';
import type { LayerNodeDto } from '../components/LayerPanel';

export async function loadImage(path: string): Promise<LoadImageResponse> {
  return invoke<LoadImageResponse>('load_image', { path });
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

export async function getLayerTree(): Promise<LayerNodeDto[]> {
  return invoke<LayerNodeDto[]>('get_layer_tree');
}

export async function addFilter(
  layerId: number,
  kind: string,
  params: Record<string, unknown>
): Promise<{ filter_id: string }> {
  return invoke<{ filter_id: string }>('add_filter', { req: { layer_id: layerId, kind, params } });
}

export async function updateFilter(
  layerId: number,
  filterId: string,
  params: Record<string, unknown>
): Promise<void> {
  return invoke<void>('update_filter', { req: { layer_id: layerId, filter_id: filterId, params } });
}

export async function removeFilter(
  layerId: number,
  filterId: string
): Promise<void> {
  return invoke<void>('remove_filter', { req: { layer_id: layerId, filter_id: filterId } });
}

export async function exportImage(req: ExportImageRequest): Promise<void> {
  return invoke<void>('export_image', { req });
}
