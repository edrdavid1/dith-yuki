/**
 * W9: TitlebarGestureController — owns undock commit + floating MOVE_NODE guard.
 *
 * On pointerdown it starts a machine session (FlexLayout native drag still
 * bubbles). Crossing EXIT_SLACK commits undock once via cancelFlexLayoutDrag
 * then onUndock. Residual MOVE_NODE of an already-floating tab is rejected
 * through filterLayoutAction (debug log).
 */

import {
  Actions,
  DragDrop,
  TabNode,
  type Action,
  type Model,
} from 'flexlayout-react';
import {
  createIdleTitlebarGesture,
  reduceTitlebarGesture,
  type TitlebarGestureMode,
  type TitlebarGestureState,
} from './titlebarGestureMachine';

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

/**
 * Layout onAction filter for floating-tab MOVE_NODE. Returns undefined to
 * reject; otherwise returns the action unchanged.
 */
export function filterLayoutAction(
  model: Model | null,
  action: Action,
): Action | undefined {
  if (!model) return action;
  if (!shouldBlockFloatingTabMove(model, action)) return action;
  const fromId = action.data?.fromNode as string | undefined;
  console.debug(
    '[flex-dock] reject MOVE_NODE for floating tab',
    fromId ?? '(unknown)',
  );
  return undefined;
}

export type TitlebarGestureControllerOptions = {
  getColumnEl: () => HTMLElement | null;
  getMode: () => TitlebarGestureMode;
  onUndock: () => void;
  /** Document used for listeners (popout-safe). Defaults to `document`. */
  getOwnerDocument?: () => Document;
};

/**
 * Session controller: wire titlebar pointerdown → machine → undock commit.
 */
export class TitlebarGestureController {
  private state: TitlebarGestureState = createIdleTitlebarGesture();
  private detach: (() => void) | null = null;

  constructor(private readonly opts: TitlebarGestureControllerOptions) {}

  /** Call from titlebar mousedown/pointerdown (primary button, non-button target). */
  onTitlebarPointerDown(e: {
    button: number;
    clientX: number;
    clientY: number;
    target: EventTarget | null;
  }): void {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest?.('button')) return;

    this.teardownListeners();
    const reduced = reduceTitlebarGesture(this.state, {
      type: 'pointerdown',
      mode: this.opts.getMode(),
      x: e.clientX,
      y: e.clientY,
    });
    this.state = reduced.next;
    this.attachListeners();
  }

  /** Expose for tests. */
  getState(): TitlebarGestureState {
    return this.state;
  }

  dispose(): void {
    this.teardownListeners();
    this.state = createIdleTitlebarGesture();
  }

  private ownerDoc(): Document {
    return this.opts.getOwnerDocument?.() ?? document;
  }

  private attachListeners(): void {
    const doc = this.ownerDoc();
    const onMove = (ev: PointerEvent) => this.onMove(ev);
    const onUp = () => this.onTerminal('pointerup');
    const onCancel = () => this.onTerminal('cancel');
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') this.onTerminal('cancel');
    };
    doc.addEventListener('pointermove', onMove);
    doc.addEventListener('pointerup', onUp);
    doc.addEventListener('pointercancel', onCancel);
    doc.addEventListener('keydown', onKey);
    this.detach = () => {
      doc.removeEventListener('pointermove', onMove);
      doc.removeEventListener('pointerup', onUp);
      doc.removeEventListener('pointercancel', onCancel);
      doc.removeEventListener('keydown', onKey);
    };
  }

  private teardownListeners(): void {
    this.detach?.();
    this.detach = null;
  }

  private onMove(ev: PointerEvent): void {
    const col = this.opts.getColumnEl();
    const rect = col?.getBoundingClientRect() ?? null;
    const column = rect
      ? {
          left: rect.left,
          right: rect.right,
          top: rect.top,
          bottom: rect.bottom,
        }
      : null;
    const { next, effect } = reduceTitlebarGesture(this.state, {
      type: 'pointermove',
      x: ev.clientX,
      y: ev.clientY,
      column,
    });
    this.state = next;
    if (effect === 'undock') {
      this.commitUndock();
    }
  }

  private onTerminal(kind: 'pointerup' | 'cancel'): void {
    const { next, effect } = reduceTitlebarGesture(this.state, { type: kind });
    this.state = next;
    this.teardownListeners();
    if (effect === 'cleanup') {
      // No undock — native FL drag completes on its own.
    }
  }

  /** Cancel native FL drag, then float exactly once. */
  private commitUndock(): void {
    this.teardownListeners();
    cancelFlexLayoutDrag();
    this.opts.onUndock();
  }
}
