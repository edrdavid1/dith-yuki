import { invoke } from '@tauri-apps/api/core';

/** Ask Rust to tear down the OS splash. Main is already visible. */
export function notifyBootFinished(): Promise<void> {
  return invoke('boot_finished');
}

/** Optional stage text on the splash (`window.setStage` via Rust eval). */
export function setBootStage(text: string): Promise<void> {
  return invoke('boot_stage', { text });
}
