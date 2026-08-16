import { describe, it, expect, vi } from 'vitest';
import { confirmAndInstallUpdate, isTooNewFileError } from '../appUpdates';

describe('isTooNewFileError', () => {
  it('matches future dyproj format_version', () => {
    expect(
      isTooNewFileError(
        'unsupported dyproj format_version 99 (this app supports up to 1); update the app'
      )
    ).toBe(true);
  });

  it('matches AppVersionTooOld', () => {
    expect(
      isTooNewFileError(
        'this pattern requires app version 0.3.0 or newer (running 0.2.0); update the app'
      )
    ).toBe(true);
  });

  it('matches unknown-enum serde', () => {
    expect(isTooNewFileError('unknown variant `FutureKind`, expected ...')).toBe(true);
  });

  it('ignores unrelated IPC errors', () => {
    expect(isTooNewFileError('Failed to open image')).toBe(false);
    expect(isTooNewFileError(null)).toBe(false);
  });
});

describe('confirmAndInstallUpdate', () => {
  it('Cancel does not download or relaunch', async () => {
    const downloadAndInstall = vi.fn();
    const relaunch = vi.fn();
    await expect(
      confirmAndInstallUpdate({
        confirmRestart: async () => false,
        update: { version: '0.2.1', notes: '', downloadAndInstall },
        relaunch,
      })
    ).resolves.toBe('cancelled');
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
  });

  it('Install after confirm downloads then relaunches', async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    const relaunch = vi.fn().mockResolvedValue(undefined);
    await expect(
      confirmAndInstallUpdate({
        confirmRestart: async () => true,
        update: { version: '0.2.1', notes: 'fixes', downloadAndInstall },
        relaunch,
      })
    ).resolves.toBe('installed');
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });
});
