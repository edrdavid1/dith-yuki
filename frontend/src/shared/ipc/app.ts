import { invoke } from '@tauri-apps/api/core';

export async function allowAppExit(): Promise<void> {
  return invoke('allow_app_exit');
}

export async function confirmAppQuit(): Promise<void> {
  return invoke('confirm_app_quit');
}

export async function stopDockBounce(): Promise<void> {
  return invoke('stop_dock_bounce');
}
