import { describe, expect, it } from 'vitest';
import {
  EXIT_SLACK_PX,
  PREVIEW_UNDOCK_DISTANCE_PX,
  UNDOCK_ARM_THRESHOLD_PX,
} from '../gestureConfig';
import {
  createIdleTitlebarGesture,
  reduceTitlebarGesture,
  type ColumnRect,
  type TitlebarGestureState,
} from '../titlebarGestureMachine';

const COLUMN: ColumnRect = {
  left: 100,
  right: 400,
  top: 0,
  bottom: 600,
};

function startSidebar(
  x = 200,
  y = 100,
): TitlebarGestureState {
  return reduceTitlebarGesture(createIdleTitlebarGesture(), {
    type: 'pointerdown',
    mode: 'sidebar',
    x,
    y,
  }).next;
}

function move(
  state: TitlebarGestureState,
  x: number,
  y: number,
  column: ColumnRect | null = COLUMN,
) {
  return reduceTitlebarGesture(state, { type: 'pointermove', x, y, column });
}

describe('titlebarGestureMachine', () => {
  it.each([
    {
      name: 'in-column move does not undock',
      x: 210,
      y: 100,
      expectUndock: false,
    },
    {
      name: 'inside exit slack band does not undock',
      x: COLUMN.left - 30,
      y: 100,
      expectUndock: false,
    },
    {
      name: 'past EXIT_SLACK left undocks once',
      x: COLUMN.left - (EXIT_SLACK_PX + 6),
      y: 100,
      expectUndock: true,
    },
    {
      name: 'past EXIT_SLACK right undocks once',
      x: COLUMN.right + (EXIT_SLACK_PX + 6),
      y: 100,
      expectUndock: true,
    },
  ])('$name', ({ x, y, expectUndock }) => {
    let state = startSidebar();
    // Arm past threshold while still inside.
    let r = move(state, 200 + UNDOCK_ARM_THRESHOLD_PX + 1, 100);
    state = r.next;
    expect(r.effect).toBe('none');

    r = move(state, x, y);
    if (expectUndock) {
      expect(r.effect).toBe('undock');
      expect(r.next.phase).toBe('undocked');
      expect(r.next.undockCount).toBe(1);
      // Further moves must not undock again.
      const again = move(r.next, x - 20, y);
      expect(again.effect).toBe('none');
      expect(again.next.undockCount).toBe(1);
    } else {
      expect(r.effect).toBe('none');
      expect(r.next.undockCount).toBe(0);
    }
  });

  it('below arm threshold stays pending without undock', () => {
    const state = startSidebar();
    const r = move(state, 200 + UNDOCK_ARM_THRESHOLD_PX - 1, 100);
    expect(r.next.phase).toBe('pending');
    expect(r.effect).toBe('none');
  });

  it('anyDirection undocks after PREVIEW_UNDOCK_DISTANCE_PX', () => {
    let state = reduceTitlebarGesture(createIdleTitlebarGesture(), {
      type: 'pointerdown',
      mode: 'anyDirection',
      x: 500,
      y: 300,
    }).next;
    const short = move(state, 500 + PREVIEW_UNDOCK_DISTANCE_PX - 1, 300, null);
    expect(short.effect).toBe('none');
    const far = move(short.next, 500 + PREVIEW_UNDOCK_DISTANCE_PX, 300, null);
    expect(far.effect).toBe('undock');
    expect(far.next.undockCount).toBe(1);
  });

  it('pointerup before undock cleans up to idle', () => {
    const state = startSidebar();
    const r = reduceTitlebarGesture(state, { type: 'pointerup' });
    expect(r.next.phase).toBe('idle');
    expect(r.effect).toBe('cleanup');
  });
});
