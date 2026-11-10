/**
 * Titlebar undock races FlexLayout's native tab drag (same mousedown bubbles to
 * TabButton). With enableEdgeDock, releasing the mouse after floatPanel can still
 * MOVE_NODE the live popout onto an edge target — the FloatingWindow remounts /
 * closes and the detached panel vanishes.
 *
 * Cancel the in-flight drag before float, and refuse Layout MOVE_NODEs of tabs
 * that are already floating (model.doAction park/redock paths bypass onAction).
 */

import { Actions, DragDrop, TabNode, type Action, type Model } from 'flexlayout-react';

type DragDropCancelable = DragDrop & {
  _onDragCancel?: () => void;
};

/** Abort FlexLayout's glass drag so mouseup cannot relocate a just-floated tab. */
export function cancelFlexLayoutDrag(): void {
  const dd = DragDrop.instance as DragDropCancelable;
  if (!dd.isActive()) return;
  // No public cancel API — Escape path is the supported internal cleanup.
  dd._onDragCancel?.();
}

/**
 * True when a Layout-originated MOVE_NODE would relocate an already-floating tab
 * (residual undock drag-end). Parking via model.doAction does not use this gate.
 */
export function shouldBlockFloatingTabMove(model: Model, action: Action): boolean {
  if (action.type !== Actions.MOVE_NODE) return false;
  const fromId = action.data?.fromNode as string | undefined;
  if (!fromId) return false;
  const node = model.getNodeById(fromId);
  if (!node || node.getType() !== TabNode.TYPE) return false;
  return (node as TabNode).isFloating();
}
