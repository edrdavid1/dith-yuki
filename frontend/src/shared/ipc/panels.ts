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
  /** Physical screen pixels (W6). */
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  side: DockSide;
  slots: DockZoneSlot[];
  /** Monotonic seq — Rust drops stale packets (W6). */
  seq?: number;
}

export async function updateDockZone(
  side: DockSide,
  zone: DockZonePayload | null,
): Promise<void> {
  return invoke<void>('update_dock_zone', { side, zone });
}

export async function beginFloatDrag(
  panelId: string,
  windowLabel?: string | null,
): Promise<void> {
  return invoke<void>('begin_float_drag', {
    panelId,
    windowLabel: windowLabel ?? null,
  });
}

/** Bind the concrete `flex-popout-*` label after the shell is resolved. */
export async function bindFloatDragWindow(windowLabel: string): Promise<void> {
  return invoke<void>('bind_float_drag_window', { windowLabel });
}

/** Cancel float-drag affinity session. Idempotent; optional label must match when set. */
export async function cancelFloatDrag(
  windowLabel?: string | null,
): Promise<void> {
  return invoke<void>('cancel_float_drag', {
    windowLabel: windowLabel ?? null,
  });
}

/**
 * End JS-driven float drag; docks if affinity armed (Flex → flex-panel-dock-request).
 * Idempotent; optional label must match the active session when set.
 */
export async function completeFloatDrag(
  windowLabel?: string | null,
): Promise<void> {
  return invoke<void>('complete_float_drag', {
    windowLabel: windowLabel ?? null,
  });
}

export type { DockSide };
