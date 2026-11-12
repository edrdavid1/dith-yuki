import { invoke } from '@tauri-apps/api/core';

export async function allowAppExit(): Promise<void> {
  return invoke('allow_app_exit');
}

export async function confirmAppQuit(): Promise<void> {
  return invoke('confirm_app_quit');
}

/** Cancel Dock attention (post-startup events). Startup uses the splash window. */
export async function stopDockBounce(): Promise<void> {
  return invoke('stop_dock_bounce');
}

/** macOS: AppKit `zoom:` (fit/restore). Other platforms: toggle maximize. */
export async function zoomWindow(): Promise<void> {
  return invoke('zoom_window');
}
