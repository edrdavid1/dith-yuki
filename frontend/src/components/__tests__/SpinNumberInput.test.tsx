import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import SpinNumberInput from '../common/SpinNumberInput';

vi.mock('../../hooks/useHapticFeedback', () => ({
  useHapticFeedback: () => ({ triggerHaptic: vi.fn() }),
}));

describe('SpinNumberInput', () => {
  it('renders value and steps with buttons', () => {
    const onChange = vi.fn();
    render(
      <SpinNumberInput label="Cell width" value={32} min={1} max={64} onChange={onChange} />,
    );
    expect(screen.getByDisplayValue('32')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Increase Cell width'));
    expect(onChange).toHaveBeenCalledWith(33);
    fireEvent.click(screen.getByLabelText('Decrease Cell width'));
    expect(onChange).toHaveBeenCalledWith(31);
  });

  it('commits typed value on blur', () => {
    const onChange = vi.fn();
    render(
      <SpinNumberInput label="Cell height" value={16} min={1} max={64} onChange={onChange} />,
    );
    const input = screen.getByLabelText('Cell height');
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '48' } });
    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledWith(48);
  });

  it('disables decrease at min', () => {
    render(
      <SpinNumberInput label="Pad" value={0} min={0} max={8} onChange={vi.fn()} />,
    );
    expect(screen.getByLabelText('Decrease Pad')).toBeDisabled();
  });
});
