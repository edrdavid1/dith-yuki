// =============================================================================
// Panel Types and Constants
// =============================================================================

export interface SavedBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type DockSide = 'left' | 'right';

export interface PanelInfo {
  id: string;
  docked: boolean;
  visible: boolean;
  window_label: string | null;
  saved_bounds: SavedBounds | null;
  /** Present when docked + dockable; null when floating or floating-only. */
  dock_side: DockSide | null;
}

export type PanelId = 'effect' | 'layers' | 'colorlab' | 'preview' | 'preferences';

/** Panels that never appear in the docked sidebar (floating-only). */
export const FLOATING_ONLY_PANELS: ReadonlySet<PanelId> = new Set(['preview', 'preferences']);

export const PANEL_IDS: PanelId[] = ['effect', 'layers', 'colorlab', 'preview', 'preferences'];

export const PANEL_DISPLAY_NAMES: Record<PanelId, string> = {
  effect: 'Effect Settings',
  layers: 'Layers',
  colorlab: 'Color Lab',
  preview: 'Preview',
  preferences: 'Preferences',
};

export const PANEL_DEFAULT_BOUNDS: Record<PanelId, { width: number; height: number }> = {
  effect: { width: 400, height: 600 },
  layers: { width: 350, height: 500 },
  colorlab: { width: 560, height: 640 },
  preview: { width: 800, height: 600 },
  preferences: { width: 420, height: 360 },
};

/** Full dual-sidebar snapshot from Rust (`get_panels_state` / panel-state-changed). */
export interface PanelStateSnapshot {
  panels: PanelInfo[];
  left_order: PanelId[];
  right_order: PanelId[];
}
