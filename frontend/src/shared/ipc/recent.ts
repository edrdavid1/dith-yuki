import { invoke } from '@tauri-apps/api/core';

export type RecentFileKind = 'image' | 'project';

export interface RecentFileEntry {
  path: string;
  kind: RecentFileKind;
  display_name: string;
  opened_at: string;
}

export async function getRecentFiles(): Promise<RecentFileEntry[]> {
  return invoke<RecentFileEntry[]>('get_recent_files');
}

export function openRecentByKind(
  entry: RecentFileEntry,
  helpers: {
    openImageAt: (path: string) => void | Promise<void>;
    openProjectAt: (path: string) => void | Promise<void>;
  }
): void | Promise<void> {
  if (entry.kind === 'image') {
    return helpers.openImageAt(entry.path);
  }
  return helpers.openProjectAt(entry.path);
}
