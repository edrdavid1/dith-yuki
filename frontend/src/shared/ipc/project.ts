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

export async function saveProject(
  docId: number,
  path?: string | null
): Promise<SaveProjectResponse> {
  return invoke<SaveProjectResponse>('save_project', {
    docId,
    path: path ?? null,
  });
}

export async function saveProjectAs(docId: number, path: string): Promise<SaveProjectResponse> {
  return invoke<SaveProjectResponse>('save_project_as', {
    docId,
    path,
  });
}

export interface ShareProjectCopyOptions {
  stripMetadata?: boolean;
  includeOriginalImages?: boolean;
  includeAuthor?: boolean;
  compact?: boolean;
}

/** Explicit privacy export (SPEC §11). Does not change the open project path. */
export async function shareProjectCopy(
  docId: number,
  path: string,
  opts?: ShareProjectCopyOptions
): Promise<SaveProjectResponse> {
  return invoke<SaveProjectResponse>('share_project_copy', {
    docId,
    path,
    opts: opts
      ? {
          strip_metadata: opts.stripMetadata,
          include_original_images: opts.includeOriginalImages,
          include_author: opts.includeAuthor,
          compact: opts.compact,
        }
      : null,
  });
}

export async function openProject(path: string): Promise<OpenProjectResponse> {
  return invoke<OpenProjectResponse>('open_project', { path });
}
