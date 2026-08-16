import type { AppUpdateHandle, DownloadProgress } from './ipc/updates';

export function isTooNewFileError(message: string | null | undefined): boolean {
  if (!message) return false;
  const m = message.toLowerCase();
  return (
    (m.includes('format_version') && m.includes('update the app')) ||
    m.includes('requires app version') ||
    m.includes('unknown variant')
  );
}

export async function confirmAndInstallUpdate(opts: {
  confirmRestart: () => Promise<boolean>;
  update: AppUpdateHandle;
  relaunch: () => Promise<void>;
  onProgress?: (event: DownloadProgress) => void;
  isCancelled?: () => boolean;
}): Promise<'cancelled' | 'installed'> {
  const ok = await opts.confirmRestart();
  if (!ok) return 'cancelled';
  await opts.update.downloadAndInstall((event) => {
    if (opts.isCancelled?.()) {
      throw new Error('Update download cancelled');
    }
    opts.onProgress?.(event);
  });
  if (opts.isCancelled?.()) return 'cancelled';
  await opts.relaunch();
  return 'installed';
}
