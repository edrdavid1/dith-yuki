import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import ColorPicker from '../ColorPicker';

function renderPicker(overrides?: Partial<React.ComponentProps<typeof ColorPicker>>) {
  const defaults = {
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
  };
  const props = { ...defaults, ...overrides };
  return { ...render(<ColorPicker {...props} />), props };
}

describe('ColorPicker', () => {
  it('renders dialog with color picker', () => {
    renderPicker();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByLabelText('Hex color value')).toBeInTheDocument();
  });

  it('defaults to FFFFFF when no initialColor provided', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    expect(input.value).toBe('FFFFFF');
  });

  it('uses initialColor when provided', () => {
    renderPicker({ initialColor: 'FF0000' });
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    expect(input.value).toBe('FF0000');
  });

  it('calls onConfirm with uppercase hex when Confirm is clicked', () => {
    const { props } = renderPicker({ initialColor: 'aabbcc' });
    fireEvent.click(screen.getByText('Confirm'));
    expect(props.onConfirm).toHaveBeenCalledWith('AABBCC');
  });

  it('calls onCancel when Cancel is clicked', () => {
    const { props } = renderPicker();
    fireEvent.click(screen.getByText('Cancel'));
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('calls onCancel when Escape is pressed', () => {
    const { props } = renderPicker();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('calls onCancel when clicking the overlay', () => {
    const { props } = renderPicker();
    const overlay = screen.getByTestId('color-picker-overlay');
    fireEvent.click(overlay);
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('does not call onCancel when clicking inside the modal', () => {
    const { props } = renderPicker();
    const dialog = screen.getByRole('dialog');
    fireEvent.click(dialog);
    expect(props.onCancel).not.toHaveBeenCalled();
  });

  it('updates hex input and filters invalid characters', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'gg12zz' } });
    // Only valid hex chars kept, uppercased
    expect(input.value).toBe('12');
  });

  it('allows editing hex input to a valid 6-char value', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: '00ff88' } });
    expect(input.value).toBe('00FF88');
  });

  it('emits the edited hex value on confirm', () => {
    const { props } = renderPicker({ initialColor: 'FFFFFF' });
    const input = screen.getByLabelText('Hex color value');
    fireEvent.change(input, { target: { value: '123abc' } });
    fireEvent.click(screen.getByText('Confirm'));
    expect(props.onConfirm).toHaveBeenCalledWith('123ABC');
  });

  it('truncates hex input to 6 characters', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'AABBCCDD' } });
    expect(input.value).toBe('AABBCC');
  });

  it('renders Confirm and Cancel buttons', () => {
    renderPicker();
    expect(screen.getByText('Confirm')).toBeInTheDocument();
    expect(screen.getByText('Cancel')).toBeInTheDocument();
  });

  it('renders color preview swatch', () => {
    renderPicker({ initialColor: 'FF0000' });
    const preview = screen.getByLabelText('Color preview');
    expect(preview).toBeInTheDocument();
  });
});
