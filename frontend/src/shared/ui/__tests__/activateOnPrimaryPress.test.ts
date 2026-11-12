import { describe, it, expect, vi } from 'vitest';
import { activateOnPrimaryPress } from '../activateOnPrimaryPress';

function fakePointer(button = 0) {
  return {
    button,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as React.PointerEvent;
}

function fakeClick() {
  return {
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as React.MouseEvent;
}

describe('activateOnPrimaryPress', () => {
  it('fires once when both pointerdown and click arrive', () => {
    const action = vi.fn();
    const handlers = activateOnPrimaryPress(action);
    handlers.onPointerDown(fakePointer());
    handlers.onClick(fakeClick());
    expect(action).toHaveBeenCalledTimes(1);
  });

  it('fires on click alone (popout first-press quirk)', () => {
    const action = vi.fn();
    const handlers = activateOnPrimaryPress(action);
    handlers.onClick(fakeClick());
    expect(action).toHaveBeenCalledTimes(1);
  });

  it('ignores non-primary pointer buttons', () => {
    const action = vi.fn();
    const handlers = activateOnPrimaryPress(action);
    handlers.onPointerDown(fakePointer(2));
    expect(action).not.toHaveBeenCalled();
  });
});
