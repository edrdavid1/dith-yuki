import { invoke } from '@tauri-apps/api/core';

export interface SaveProjectResponse {
  path: string;
  size_warning: boolean;
}

export interface OpenProjectResponse {
  doc_id: number;
  width: number;
  height: number;
  path: string;
}

export async function saveProject(path?: string | null): Promise<SaveProjectResponse> {
  return invoke<SaveProjectResponse>('save_project', { path: path ?? null });
}

export async function saveProjectAs(path: string): Promise<SaveProjectResponse> {
  return invoke<SaveProjectResponse>('save_project_as', { path });
}

export async function openProject(path: string): Promise<OpenProjectResponse> {
  return invoke<OpenProjectResponse>('open_project', { path });
}
