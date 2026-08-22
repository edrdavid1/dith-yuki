import { invoke } from '@tauri-apps/api/core';

export async function allowAppExit(): Promise<void> {
  return invoke('allow_app_exit');
}

export async function confirmAppQuit(): Promise<void> {
  return invoke('confirm_app_quit');
}

export type GpuPreviewStatus = {
  enabled: boolean;
  available: boolean;
  /** `DITHER_GPU_PREVIEW` env set — Preferences cannot override until unset. */
  envForced: boolean;
};

export async function getGpuPreviewStatus(): Promise<GpuPreviewStatus> {
  return invoke('get_gpu_preview_status');
}

export async function setGpuPreviewEnabled(enabled: boolean): Promise<GpuPreviewStatus> {
  return invoke('set_gpu_preview_enabled', { enabled });
}
