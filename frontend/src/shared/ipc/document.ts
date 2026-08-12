import { invoke } from '@tauri-apps/api/core';
import type { LoadImageResponse, ExportImageRequest } from '../../types';

/** Document snapshot response from `get_document_snapshot`. */
export interface DocumentSnapshotResponse {
  snapshot: {
    id?: number;
    width?: number;
    height?: number;
    layers: SnapshotLayerNode[];
  };
}

export interface SnapshotLayerNode {
  kind?: string;
  id?: { inner: number } | number;
  filters?: SnapshotFilterInfo[];
  children?: SnapshotLayerNode[];
  [key: string]: unknown;
}

export interface SnapshotFilterInfo {
  id: string;
  kind: string;
  params: Record<string, unknown>;
  enabled: boolean;
}

export async function loadImage(path: string): Promise<LoadImageResponse> {
  return invoke<LoadImageResponse>('load_image', { path });
}

export async function exportImage(req: ExportImageRequest): Promise<void> {
  return invoke<void>('export_image', { req });
}

export async function getDocumentSnapshot(): Promise<DocumentSnapshotResponse> {
  return invoke<DocumentSnapshotResponse>('get_document_snapshot');
}
