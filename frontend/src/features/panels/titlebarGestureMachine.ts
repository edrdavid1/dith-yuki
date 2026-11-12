/**
 * W9: pure titlebar gesture state machine.
 *
 * idle → pending → (armed) → undocked | idle
 *
 * On pointerdown the session starts in `pending` while FlexLayout's native tab
 * drag also runs (same mousedown). Crossing EXIT_SLACK outside the column
 * commits undock once; in-column moves never undock.
 */

import {
  EXIT_SLACK_PX,
  PREVIEW_UNDOCK_DISTANCE_PX,
  UNDOCK_ARM_THRESHOLD_PX,
} from './gestureConfig';

export type TitlebarGestureMode = 'sidebar' | 'anyDirection';

export type TitlebarGesturePhase =
  | 'idle'
  | 'pending'
  | 'armed'
  | 'undocked';

export type ColumnRect = {
  left: number;
  right: number;
  top: number;
  bottom: number;
};

export type TitlebarGestureState = {
  phase: TitlebarGesturePhase;
  mode: TitlebarGestureMode;
  startX: number;
  startY: number;
  /** How many times undock was committed (0 or 1 for a healthy session). */
  undockCount: number;
};

export type TitlebarGestureEvent =
  | {
      type: 'pointerdown';
      mode: TitlebarGestureMode;
      x: number;
      y: number;
    }
  | {
      type: 'pointermove';
      x: number;
      y: number;
      column: ColumnRect | null;
    }
  | { type: 'pointerup' }
  | { type: 'cancel' };

export type TitlebarGestureEffect = 'none' | 'undock' | 'cleanup';

export function createIdleTitlebarGesture(): TitlebarGestureState {
  return {
    phase: 'idle',
    mode: 'sidebar',
    startX: 0,
    startY: 0,
    undockCount: 0,
  };
}

function dist(state: TitlebarGestureState, x: number, y: number): number {
  return Math.hypot(x - state.startX, y - state.startY);
}

function outsideColumnSlack(
  x: number,
  column: ColumnRect,
  slack: number = EXIT_SLACK_PX,
): boolean {
  return x < column.left - slack || x > column.right + slack;
}

/**
 * Reduce one gesture event. Side effects (`undock` / `cleanup`) are hints for
 * the controller — the machine itself is pure.
 */
export function reduceTitlebarGesture(
  state: TitlebarGestureState,
  event: TitlebarGestureEvent,
): { next: TitlebarGestureState; effect: TitlebarGestureEffect } {
  switch (event.type) {
    case 'pointerdown': {
      return {
        next: {
          phase: 'pending',
          mode: event.mode,
          startX: event.x,
          startY: event.y,
          undockCount: 0,
        },
        effect: 'none',
      };
    }
    case 'pointermove': {
      if (state.phase === 'idle' || state.phase === 'undocked') {
        return { next: state, effect: 'none' };
      }

      if (state.mode === 'anyDirection') {
        if (dist(state, event.x, event.y) >= PREVIEW_UNDOCK_DISTANCE_PX) {
          return {
            next: {
              ...state,
              phase: 'undocked',
              undockCount: state.undockCount + 1,
            },
            effect: 'undock',
          };
        }
        return { next: state, effect: 'none' };
      }

      // sidebar mode
      let phase = state.phase;
      if (phase === 'pending') {
        if (dist(state, event.x, event.y) < UNDOCK_ARM_THRESHOLD_PX) {
          return { next: state, effect: 'none' };
        }
        phase = 'armed';
      }

      if (phase === 'armed' && event.column) {
        if (outsideColumnSlack(event.x, event.column)) {
          return {
            next: {
              ...state,
              phase: 'undocked',
              undockCount: state.undockCount + 1,
            },
            effect: 'undock',
          };
        }
      }

      return { next: { ...state, phase }, effect: 'none' };
    }
    case 'pointerup':
    case 'cancel': {
      if (state.phase === 'idle') {
        return { next: state, effect: 'none' };
      }
      return {
        next: createIdleTitlebarGesture(),
        effect: state.phase === 'undocked' ? 'none' : 'cleanup',
      };
    }
    default: {
      return { next: state, effect: 'none' };
    }
  }
}

/** Test helper: would a move at (x,y) undock given current state + column? */
export function wouldUndockOnMove(
  state: TitlebarGestureState,
  x: number,
  y: number,
  column: ColumnRect | null,
): boolean {
  return reduceTitlebarGesture(state, {
    type: 'pointermove',
    x,
    y,
    column,
  }).effect === 'undock';
}
