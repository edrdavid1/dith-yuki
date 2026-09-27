import { invoke } from '@tauri-apps/api/core';

export interface AppIconVariant {
  id: string;
  label: string;
  previewSrc: string;
  todoDesign: boolean;
}

export interface AppIconState {
  id: string;
  persistentOk: boolean;
}

export interface AppIconApplyResult {
  id: string;
  persistentOk: boolean;
  warning: string | null;
}

export async function listAppIcons(): Promise<AppIconVariant[]> {
  return invoke<AppIconVariant[]>('list_app_icons');
}

export async function getAppIcon(): Promise<AppIconState> {
  return invoke<AppIconState>('get_app_icon');
}

export async function setAppIcon(id: string): Promise<AppIconApplyResult> {
  return invoke<AppIconApplyResult>('set_app_icon', { id });
}
