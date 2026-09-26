import { invoke } from '@tauri-apps/api/core';
import type { DockSide } from '../../types/panels';

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

/** End JS-driven float drag; docks if affinity armed (Flex → flex-panel-dock-request). */
export async function completeFloatDrag(): Promise<void> {
  return invoke<void>('complete_float_drag');
}

export type { DockSide };
