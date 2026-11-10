import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useRef } from 'react';
import { useFlexTitlebarUndock } from '../useFlexTitlebarUndock';

const cancelFlexLayoutDrag = vi.hoisted(() => vi.fn());

vi.mock('../flexFloatDragGuard', () => ({
  cancelFlexLayoutDrag,
}));

describe('useFlexTitlebarUndock', () => {
  beforeEach(() => {
    cancelFlexLayoutDrag.mockClear();
  });

  it('floats once the pointer leaves the column by the exit slack', () => {
    const onUndock = vi.fn();
    const column = document.createElement('div');
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

    // Arm with a small move still inside.
    document.dispatchEvent(
      new MouseEvent('mousemove', { clientX: 210, clientY: 100 }),
    );
    expect(onUndock).not.toHaveBeenCalled();
    expect(cancelFlexLayoutDrag).not.toHaveBeenCalled();

    // Still inside the slack band — do not undock yet.
    document.dispatchEvent(
      new MouseEvent('mousemove', { clientX: 100 - 30, clientY: 100 }),
    );
    expect(onUndock).not.toHaveBeenCalled();

    // Leave past the left edge (+64px slack).
    document.dispatchEvent(
      new MouseEvent('mousemove', { clientX: 100 - 70, clientY: 100 }),
    );
    expect(onUndock).toHaveBeenCalledTimes(1);
    // Native FL drag must be cancelled before floatPanel opens the popout.
    expect(cancelFlexLayoutDrag).toHaveBeenCalledTimes(1);
    expect(cancelFlexLayoutDrag.mock.invocationCallOrder[0]!).toBeLessThan(
      onUndock.mock.invocationCallOrder[0]!,
    );

    document.body.removeChild(column);
  });
});
