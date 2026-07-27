import { invoke } from '@tauri-apps/api/core';
import type { LoadImageResponse, RenderPreviewResponse, ExportImageRequest } from '../types';

export async function loadImage(path: string): Promise<LoadImageResponse> {
  return invoke<LoadImageResponse>('load_image', { path });
}

export async function renderPreview(docId: number): Promise<RenderPreviewResponse> {
  return invoke<RenderPreviewResponse>('render_preview', { docId });
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
