import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import NumberInput from '../common/NumberInput';
import { clampAndSnap, formatValue } from '../common/Slider';

describe('clampAndSnap', () => {
  it('clamps below min and above max', () => {
    expect(clampAndSnap(-5, 0, 10, 1)).toBe(0);
    expect(clampAndSnap(15, 0, 10, 1)).toBe(10);
  });

  it('snaps to step', () => {
    expect(clampAndSnap(0.24, 0, 1, 0.1)).toBe(0.2);
    expect(clampAndSnap(0.26, 0, 1, 0.1)).toBe(0.3);
  });
});

describe('formatValue', () => {
  it('formats to the requested decimal places', () => {
    expect(formatValue(1, 0)).toBe('1');
    expect(formatValue(0.5, 2)).toBe('0.50');
  });
});

describe('NumberInput', () => {
  it('commits clamped value on blur', () => {
    const onChange = vi.fn();
    render(
      <NumberInput label="Seed" value={0} min={0} max={99999} step={1} onChange={onChange} />,
    );
    const input = screen.getByLabelText('Seed');
    fireEvent.change(input, { target: { value: '150000' } });
    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledWith(99999);
  });

  it('commits on Enter', () => {
    const onChange = vi.fn();
    render(
      <NumberInput label="Seed" value={10} min={0} max={99999} step={1} onChange={onChange} />,
    );
    const input = screen.getByLabelText('Seed');
    fireEvent.change(input, { target: { value: '42' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onChange).toHaveBeenCalledWith(42);
  });

  it('reverts invalid text on blur', () => {
    const onChange = vi.fn();
    render(
      <NumberInput label="Seed" value={7} min={0} max={99} step={1} onChange={onChange} />,
    );
    const input = screen.getByLabelText('Seed') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'abc' } });
    fireEvent.blur(input);
    expect(onChange).not.toHaveBeenCalled();
    expect(input.value).toBe('7');
  });

  it('commits clamped compact fields on blur', () => {
    const onChange = vi.fn();
    render(
      <NumberInput
        label="Point 1 X"
        value={0}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        compact
        onChange={onChange}
      />,
    );
    const input = screen.getByLabelText('Point 1 X');
    fireEvent.change(input, { target: { value: '1.5' } });
    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledWith(1);
  });
});
