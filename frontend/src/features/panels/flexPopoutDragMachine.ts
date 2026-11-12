/**
 * W1: pure state machine for flex-popout JS drag.
 * idle → dragging → (completing | cancelled) → idle
 *
 * Side effects (capture, IPC, setPosition) live in useFlexPopoutJsDrag;
 * this module only decides transitions so they can be unit-tested.
 */

export type DragPhase = 'idle' | 'dragging' | 'completing' | 'cancelled';

export type FinishReason =
  | 'pointerup'
  | 'buttons-zero'
  | 'pointercancel'
  | 'lostpointercapture'
  | 'blur'
  | 'visibilityhidden'
  | 'escape'
  | 'watchdog'
  | 'hard-cap'
  | 'unmount'
  | 'start-failed'
  | 'superseded';

/** No pointermove for this long → cancel (W1 watchdog). */
export const WATCHDOG_MS = 1500;
/** Absolute drag duration cap (W1). */
export const HARD_CAP_MS = 60_000;

export type MachineState = {
  phase: DragPhase;
  /** How many times finish was accepted from `dragging`. */
  finishCount: number;
  lastReason: FinishReason | null;
};

export function createIdleMachine(): MachineState {
  return { phase: 'idle', finishCount: 0, lastReason: null };
}

/** Completing reasons redock if affinity armed; everything else cancels. */
export function isCompletingReason(reason: FinishReason): boolean {
  return reason === 'pointerup' || reason === 'buttons-zero';
}

export function beginDrag(state: MachineState): MachineState {
  // Supersede any prior phase — a new pointerdown always starts clean.
  return { phase: 'dragging', finishCount: 0, lastReason: null };
}

/**
 * Idempotent finish. Only the first call from `dragging` takes effect.
 * Returns whether side effects should run and whether redock should complete.
 */
export function applyFinish(
  state: MachineState,
  reason: FinishReason,
): {
  next: MachineState;
  didFinish: boolean;
  shouldComplete: boolean;
} {
  if (state.phase !== 'dragging') {
    return { next: state, didFinish: false, shouldComplete: false };
  }
  const shouldComplete = isCompletingReason(reason);
  return {
    next: {
      phase: shouldComplete ? 'completing' : 'cancelled',
      finishCount: state.finishCount + 1,
      lastReason: reason,
    },
    didFinish: true,
    shouldComplete,
  };
}

/** After IPC settles, return to idle. */
export function settleToIdle(state: MachineState): MachineState {
  if (state.phase === 'completing' || state.phase === 'cancelled') {
    return { ...state, phase: 'idle' };
  }
  return state;
}
