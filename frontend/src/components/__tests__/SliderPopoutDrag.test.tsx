import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { mockTriggerHaptic } = vi.hoisted(() => ({
  mockTriggerHaptic: vi.fn(),
}));

vi.mock('../../hooks/useHapticFeedback', () => ({
  useHapticFeedback: () => ({
    triggerHaptic: mockTriggerHaptic,
    prefEnabled: true,
  }),
}));

import Slider from '../common/Slider';

function rect(width: number): DOMRect {
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: width,
    bottom: 20,
    width,
    height: 20,
    toJSON() {
      return {};
    },
  } as DOMRect;
}

describe('Slider drag with pointer capture', () => {
  let pop: Document;
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    mockTriggerHaptic.mockReset();
    pop = document.implementation.createHTMLDocument('popout');
    host = pop.createElement('div');
    pop.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
  });

  it('follows pointer moves on the track and fires haptics while dragging', () => {
    const onChange = vi.fn();
    act(() => {
      root.render(
        <Slider
          label="Amount"
          value={0}
          min={0}
          max={100}
          step={1}
          decimals={0}
          onChange={onChange}
        />,
      );
    });

    const track = host.querySelector('img')?.parentElement?.parentElement as HTMLDivElement;
    expect(track).toBeTruthy();
    track.getBoundingClientRect = () => rect(200);

    act(() => {
      track.dispatchEvent(
        new PointerEvent('pointerdown', {
          bubbles: true,
          cancelable: true,
          button: 0,
          pointerId: 1,
          clientX: 0,
        }),
      );
    });
    const pressed = onChange.mock.calls.at(-1)?.[0];
    onChange.mockClear();
    mockTriggerHaptic.mockClear();

    act(() => {
      track.dispatchEvent(
        new PointerEvent('pointermove', {
          bubbles: true,
          pointerId: 1,
          clientX: 200,
        }),
      );
    });
    expect(onChange).toHaveBeenCalled();
    expect(onChange.mock.calls.at(-1)?.[0]).not.toBe(pressed);
    expect(mockTriggerHaptic.mock.calls.length).toBeGreaterThan(0);

    act(() => {
      track.dispatchEvent(
        new PointerEvent('pointerup', {
          bubbles: true,
          pointerId: 1,
          clientX: 200,
        }),
      );
    });
    onChange.mockClear();
    mockTriggerHaptic.mockClear();

    act(() => {
      track.dispatchEvent(
        new PointerEvent('pointermove', {
          bubbles: true,
          pointerId: 1,
          clientX: 40,
        }),
      );
    });
    expect(onChange).not.toHaveBeenCalled();
    expect(mockTriggerHaptic).not.toHaveBeenCalled();
  });
});
