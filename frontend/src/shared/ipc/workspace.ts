import { invoke } from '@tauri-apps/api/core';
import type { UiVisibility, Workspace } from '../workspace/visibility';

export type WorkspaceSwitchSource = 'settings' | 'prompt' | 'hint';

export interface AdvancedConfigDto {
  tiling_advanced: boolean;
}

export interface ExperimentalConfigDto {
  /** Snake_case from nested Rust `ExperimentalConfig`. */
  gpu_dither_families?: boolean;
  /** CamelCase if the DTO rename ever applies to nested fields. */
  gpuDitherFamilies?: boolean;
}

export interface AppConfigDto {
  schemaVersion: number;
  workspace: Workspace;
  advanced: AdvancedConfigDto;
  experimental?: ExperimentalConfigDto;
  workspaceChoiceRemembered: boolean;
  workspacePromptLaunches: number;
  hasExportedTiles: boolean;
  freshInstall: boolean;
  showWorkspacePrompt: boolean;
  visibility: UiVisibility;
}

export async function getAppConfig(): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('get_app_config');
}

export async function setWorkspace(
  workspace: Workspace,
  source: WorkspaceSwitchSource = 'settings',
): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('set_workspace', { req: { workspace, source } });
}

export async function setTilingAdvanced(enabled: boolean): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('set_tiling_advanced', { req: { enabled } });
}

export async function resolveWorkspacePrompt(
  choice: Workspace,
  remember: boolean,
): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('resolve_workspace_prompt', {
    req: { choice, remember },
  });
}

export async function noteWorkspacePromptShown(): Promise<void> {
  return invoke('note_workspace_prompt_shown');
}

export async function evaluateWorkspaceHint(args: {
  trigger: string;
  filename?: string;
  docWidth?: number;
  docHeight?: number;
  edEnabled?: boolean;
}): Promise<{ show: boolean; trigger: string }> {
  return invoke('evaluate_workspace_hint', {
    req: {
      trigger: args.trigger,
      filename: args.filename ?? null,
      docWidth: args.docWidth ?? null,
      docHeight: args.docHeight ?? null,
      edEnabled: args.edEnabled ?? null,
    },
  });
}

export async function resolveWorkspaceHint(
  action: 'switch' | 'dismiss' | 'timeout',
): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('resolve_workspace_hint', { req: { action } });
}

export async function noteTileExport(): Promise<void> {
  return invoke('note_tile_export');
}
