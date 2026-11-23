import { invoke } from '@tauri-apps/api/core';

/** Mirrors `engine_project::GamedevContext` (snake_case JSON from serde). */
export type GamedevJob = 'none' | 'tile' | 'sprite' | 'animation';

export interface TileSettings {
  enabled: boolean;
}

export interface CellSettings {
  w: number;
  h: number;
  padding: number;
  independent: boolean;
}

export type PaletteSource =
  | 'per_image'
  | { shared: { set_id: string } }
  | { fixed: { palette_id: number } };

export type MaskPolicy =
  | 'free'
  | { fixed: { kind: 'ordered' | 'blue_noise'; phase: number } };

export interface ExportDefaults {
  png8: boolean;
  naming?: string | null;
  folder?: string | null;
}

export interface AlphaPolicy {
  threshold: number;
  no_dither_alpha: boolean;
  clear_rgb_in_transparent: boolean;
}

export interface PixelPerfect {
  target_res?: [number, number] | null;
  integer_upscale?: number | null;
  nearest_preview: boolean;
}

export interface GamedevContext {
  job: GamedevJob;
  tile: TileSettings;
  cell?: CellSettings | null;
  palette: PaletteSource;
  threshold_mask: MaskPolicy;
  export: ExportDefaults;
  alpha: AlphaPolicy;
  pixel_perfect: PixelPerfect;
  frame_set: string[];
}

export type GamedevCacheLayer = 'render' | 'export' | 'chips';

export type GamedevChipId = 'seamless' | 'cells' | 'stable' | 'palette' | 'alpha';
export type ChipState = 'pending' | 'ok' | 'warn' | 'error';

/** Mirrors `engine_project::chip_action`. */
export const CHIP_ACTION = {
  disableTile: 'disable_tile',
  enableWrapPadding: 'enable_wrap_padding',
  snapToPeriod: 'snap_to_period',
  cropToCellGrid: 'crop_to_cell_grid',
  switchToFrameStable: 'switch_to_frame_stable',
  thresholdAlpha: 'threshold_alpha',
} as const;

export type ChipActionId = (typeof CHIP_ACTION)[keyof typeof CHIP_ACTION];

export interface GamedevChip {
  id: GamedevChipId;
  state: ChipState;
  label: string;
  tooltip: string;
  actionLabel?: string | null;
  actionId?: string | null;
}

export interface SetGamedevContextResponse {
  context: GamedevContext;
  invalidated: GamedevCacheLayer[];
}

export interface GamedevChipActionResponse {
  context: GamedevContext;
  snap?: [number, number] | null;
}

/** Preview display mode — orthogonal to Pipeline Tile. */
export type PreviewViewMode = 'normal' | 'repeat_3x3' | 'cell_grid';

export function defaultGamedevContext(): GamedevContext {
  return {
    job: 'none',
    tile: { enabled: false },
    cell: null,
    palette: 'per_image',
    threshold_mask: 'free',
    export: { png8: false },
    alpha: {
      threshold: 0.5,
      no_dither_alpha: true,
      clear_rgb_in_transparent: true,
    },
    pixel_perfect: { nearest_preview: false },
    frame_set: [],
  };
}

export async function getGamedevContext(docId: number): Promise<GamedevContext> {
  return invoke<GamedevContext>('get_gamedev_context', {
    req: { docId },
  });
}

export async function setGamedevContext(
  docId: number,
  context: GamedevContext,
): Promise<SetGamedevContextResponse> {
  return invoke<SetGamedevContextResponse>('set_gamedev_context', {
    req: { docId, context },
  });
}

export async function getGamedevChips(docId: number): Promise<GamedevChip[]> {
  return invoke<GamedevChip[]>('get_gamedev_chips', {
    req: { docId },
  });
}

export async function refreshGamedevChips(docId: number): Promise<void> {
  return invoke('refresh_gamedev_chips', {
    req: { docId },
  });
}

export async function gamedevChipAction(
  docId: number,
  actionId: string,
): Promise<GamedevChipActionResponse> {
  return invoke<GamedevChipActionResponse>('gamedev_chip_action', {
    req: { docId, actionId },
  });
}

export interface CollectGamedevFrameSetResponse {
  context: GamedevContext;
  frameCount: number;
}

export async function collectGamedevFrameSet(
  docId: number,
  folder: string,
): Promise<CollectGamedevFrameSetResponse> {
  return invoke<CollectGamedevFrameSetResponse>('collect_gamedev_frame_set', {
    req: { docId, folder },
  });
}

export interface FrameSetReport {
  frameCount: number;
  uniqueColors: number;
  sharedPaletteLen: number;
  framesSkippedIdentical: number;
  paletteOk: boolean;
}

export interface ApplyGamedevFrameSetResponse {
  succeeded: number;
  failed: number;
  report: FrameSetReport;
}

export async function applyGamedevFrameSet(
  docId: number,
  outputDir: string,
  opts?: { layerId?: number | null; nameTemplate?: string; format?: 'png' | 'png8' },
): Promise<ApplyGamedevFrameSetResponse> {
  return invoke<ApplyGamedevFrameSetResponse>('apply_gamedev_frame_set', {
    req: {
      docId,
      layerId: opts?.layerId ?? null,
      outputDir,
      nameTemplate: opts?.nameTemplate ?? '{name}_dithered.png',
      format: opts?.format ?? 'png',
    },
  });
}
