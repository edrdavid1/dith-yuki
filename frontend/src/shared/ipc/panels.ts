import { invoke } from '@tauri-apps/api/core';
import type { DockSide, PanelInfo, PanelStateSnapshot } from '../../types/panels';

export async function getPanelsState(): Promise<PanelStateSnapshot> {
  return invoke<PanelStateSnapshot>('get_panels_state');
}

export async function undockPanel(panelId: string): Promise<void> {
  return invoke<void>('undock_panel', { panelId });
}

export async function dockPanel(panelId: string): Promise<void> {
  return invoke<void>('dock_panel', { panelId });
}

export async function hidePanel(panelId: string): Promise<void> {
  return invoke<void>('hide_panel', { panelId });
}

export async function showPanel(panelId: string): Promise<void> {
  return invoke<void>('show_panel', { panelId });
}

export async function savePanelBounds(
  panelId: string,
  x: number,
  y: number,
  width: number,
  height: number
): Promise<void> {
  return invoke<void>('save_panel_bounds', { panelId, x, y, width, height });
}

export async function undockPanelWithSize(
  panelId: string,
  width: number,
  height: number,
  x: number,
  y: number
): Promise<void> {
  return invoke<void>('undock_panel_with_size', { panelId, width, height, x, y });
}

export async function reorderSidebar(side: DockSide, order: string[]): Promise<void> {
  return invoke<void>('reorder_sidebar', { side, order });
}

/** @deprecated Use `reorderSidebar` — kept for transitional call sites. */
export async function reorderPanels(side: DockSide, order: string[]): Promise<void> {
  return reorderSidebar(side, order);
}

export async function movePanelToSide(
  panelId: string,
  side: DockSide,
  insertIndex?: number
): Promise<void> {
  return invoke<void>('move_panel_to_side', {
    panelId,
    side,
    insertIndex: insertIndex ?? null,
  });
}

export async function moveAllPanelsToSide(side: DockSide): Promise<void> {
  return invoke<void>('move_all_panels_to_side', { side });
}

export interface DockZoneSlot {
  midY: number;
  /** Top edge of the panel slot in screen logical px (for gap hit-test). */
  top: number;
  /** Bottom edge of the panel slot in screen logical px (for gap hit-test). */
  bottom: number;
}

export interface DockZonePayload {
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  side: DockSide;
  slots: DockZoneSlot[];
}

export async function updateDockZone(
  side: DockSide,
  zone: DockZonePayload | null
): Promise<void> {
  return invoke<void>('update_dock_zone', { side, zone });
}

export async function beginFloatDrag(panelId: string): Promise<void> {
  return invoke<void>('begin_float_drag', { panelId });
}

export async function cancelFloatDrag(): Promise<void> {
  return invoke<void>('cancel_float_drag');
}

export async function dockPanelAt(
  panelId: string,
  side: DockSide,
  insertIndex: number
): Promise<void> {
  return invoke<void>('dock_panel_at', { panelId, side, insertIndex });
}

// Re-export types used by callers importing from this module.
export type { DockSide, PanelInfo, PanelStateSnapshot };
