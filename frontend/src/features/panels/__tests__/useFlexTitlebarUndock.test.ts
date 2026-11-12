/**
 * Compatibility: deprecated hook name still undocks via the W9 controller.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useRef } from 'react';
import { DragDrop } from 'flexlayout-react';
import { EXIT_SLACK_PX } from '../gestureConfig';
import { useFlexTitlebarUndock } from '../useFlexTitlebarUndock';
import { cancelFlexLayoutDrag } from '../titlebarGestureController';

describe('useFlexTitlebarUndock (compat → TitlebarGestureController)', () => {
  let column: HTMLDivElement;

  beforeEach(() => {
    cancelFlexLayoutDrag();
    column = document.createElement('div');
    document.body.appendChild(column);
    column.getBoundingClientRect = () =>
      ({
        left: 100,
        right: 400,
        top: 0,
        bottom: 600,
        width: 300,
        height: 600,
        x: 100,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
  });

  afterEach(() => {
    column.remove();
  });

  it('floats once past EXIT_SLACK and cancels native FL drag first', () => {
    const onUndock = vi.fn();
    const cancelSpy = vi.fn();
    DragDrop.instance.startDrag(
      undefined,
      () => true,
      () => {},
      () => {},
      cancelSpy,
    );

    const { result } = renderHook(() => {
      const columnRef = useRef<HTMLElement | null>(column);
      return useFlexTitlebarUndock({ columnRef, onUndock });
    });

    result.current({
      button: 0,
      clientX: 200,
      clientY: 100,
      target: column,
    } as unknown as React.MouseEvent);

    document.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 210, clientY: 100 }),
    );
    expect(onUndock).not.toHaveBeenCalled();

    document.dispatchEvent(
      new PointerEvent('pointermove', {
        clientX: 100 - (EXIT_SLACK_PX + 6),
        clientY: 100,
      }),
    );
    expect(onUndock).toHaveBeenCalledTimes(1);
    expect(cancelSpy).toHaveBeenCalled();
    expect(cancelSpy.mock.invocationCallOrder[0]!).toBeLessThan(
      onUndock.mock.invocationCallOrder[0]!,
    );
  });
});
