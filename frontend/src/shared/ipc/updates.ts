import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { formatIpcError } from './errors';

export type DownloadProgress = {
  contentLength: number | null;
  downloaded: number;
};

export type AppUpdateHandle = {
  version: string;
  notes: string;
  downloadAndInstall: (
    onEvent?: (event: DownloadProgress) => void
  ) => Promise<void>;
};

export type UpdateCheckResult =
  | { status: 'none' }
  | { status: 'available'; update: AppUpdateHandle }
  | { status: 'error'; message: string };

export async function getAppVersion(): Promise<string> {
  return getVersion();
}

export async function isReleaseBuild(): Promise<boolean> {
  return invoke<boolean>('is_release_build');
}

export async function checkForAppUpdate(): Promise<UpdateCheckResult> {
  try {
    const found = await check();
    if (!found) return { status: 'none' };

    let downloaded = 0;
    let contentLength: number | null = null;

    return {
      status: 'available',
      update: {
        version: found.version,
        notes: found.body ?? '',
        downloadAndInstall: async (onEvent) => {
          await found.downloadAndInstall((event) => {
            if (event.event === 'Started') {
              contentLength = event.data.contentLength ?? null;
              downloaded = 0;
            } else if (event.event === 'Progress') {
              downloaded += event.data.chunkLength;
            }
            onEvent?.({ contentLength, downloaded });
          });
        },
      },
    };
  } catch (err) {
    return { status: 'error', message: formatIpcError(err) };
  }
}

export async function relaunchApp(): Promise<void> {
  await relaunch();
}
