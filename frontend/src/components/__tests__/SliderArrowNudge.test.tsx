import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { ReactNode } from 'react';
import { ShellProvider } from '../../app/shell/ShellContext';

const { mockTriggerHaptic } = vi.hoisted(() => ({
  mockTriggerHaptic: vi.fn(),
}));

vi.mock('../../hooks/useHapticFeedback', () => ({
  useHapticFeedback: () => ({
    triggerHaptic: mockTriggerHaptic,
    checkSupport: vi.fn(),
    prefEnabled: true,
  }),
}));

import Slider from '../common/Slider';
import NumberInput from '../common/NumberInput';

function wrapper({ children }: { children: ReactNode }) {
  return <ShellProvider>{children}</ShellProvider>;
}

describe('Slider value-field arrow keys', () => {
  beforeEach(() => {
    mockTriggerHaptic.mockReset();
  });

  it('ArrowUp/Down nudge by display precision and sync slider', () => {
    const onChange = vi.fn();
    render(
      <Slider label="Levels" value={42} min={0} max={100} step={1} decimals={0} onChange={onChange} />,
      { wrapper }
    );
    const input = screen.getByLabelText('Levels value') as HTMLInputElement;
    input.focus();
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(onChange).toHaveBeenLastCalledWith(43);
    expect(input.value).toBe('43');

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(onChange).toHaveBeenLastCalledWith(42);
  });

  it('nudges fractional display by 10^-n', () => {
    const onChange = vi.fn();
    render(
      <Slider
        label="Gamma"
        value={0.25}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={onChange}
      />,
      { wrapper }
    );
    const input = screen.getByLabelText('Gamma value');
    fireEvent.focus(input);
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(onChange).toHaveBeenLastCalledWith(0.26);
  });

  it('preventDefault on arrows even when text is invalid', () => {
    const onChange = vi.fn();
    render(
      <Slider label="X" value={1} min={0} max={10} step={1} decimals={0} onChange={onChange} />,
      { wrapper }
    );
    const input = screen.getByLabelText('X value');
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'abc' } });
    const event = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true });
    const prevented = !input.dispatchEvent(event) || event.defaultPrevented;
    expect(prevented).toBe(true);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('triggers alignment haptic on nudge', () => {
    render(
      <Slider label="Y" value={5} min={0} max={10} step={1} decimals={0} onChange={vi.fn()} />,
      { wrapper }
    );
    fireEvent.keyDown(screen.getByLabelText('Y value'), { key: 'ArrowUp' });
    expect(mockTriggerHaptic).toHaveBeenCalledWith('alignment');
  });
});

describe('NumberInput arrow keys', () => {
  it('ArrowUp increments integer seed', () => {
    const onChange = vi.fn();
    render(
      <NumberInput label="Seed" value={42} min={0} max={99} step={1} onChange={onChange} />,
      { wrapper }
    );
    const input = screen.getByLabelText('Seed');
    fireEvent.focus(input);
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(onChange).toHaveBeenCalledWith(43);
  });
});
