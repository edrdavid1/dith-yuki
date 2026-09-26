import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
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

  it('emits live onConfirm when hex reaches 6 chars', () => {
    const { props } = renderPicker({ initialColor: 'FFFFFF' });
    const input = screen.getByLabelText('Hex color value');
    fireEvent.change(input, { target: { value: 'aabbcc' } });
    expect(props.onConfirm).toHaveBeenCalledWith('AABBCC');
  });

  it('calls onCancel when Escape is pressed', () => {
    const { props } = renderPicker();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('calls onCancel when mousedown is outside the popup', async () => {
    const { props } = renderPicker();
    await waitFor(() => {
      fireEvent.mouseDown(document.body);
      expect(props.onCancel).toHaveBeenCalled();
    });
  });

  it('does not call onCancel when clicking inside the modal', async () => {
    const { props } = renderPicker();
    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });
    fireEvent.mouseDown(screen.getByRole('dialog'));
    expect(props.onCancel).not.toHaveBeenCalled();
  });

  it('updates hex input and filters invalid characters', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'gg12zz' } });
    expect(input.value).toBe('12');
  });

  it('allows editing hex input to a valid 6-char value', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: '00ff88' } });
    expect(input.value).toBe('00FF88');
  });

  it('emits the edited hex value live (no Confirm button)', () => {
    const { props } = renderPicker({ initialColor: 'FFFFFF' });
    const input = screen.getByLabelText('Hex color value');
    fireEvent.change(input, { target: { value: '123abc' } });
    expect(props.onConfirm).toHaveBeenCalledWith('123ABC');
  });

  it('truncates hex input to 6 characters', () => {
    renderPicker();
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'AABBCCDD' } });
    expect(input.value).toBe('AABBCC');
  });

  it('has no Confirm/Cancel buttons (live popup)', () => {
    renderPicker();
    expect(screen.queryByText('Confirm')).not.toBeInTheDocument();
    expect(screen.queryByText('Cancel')).not.toBeInTheDocument();
  });

  it('renders color preview swatch', () => {
    renderPicker({ initialColor: 'FF0000' });
    const preview = screen.getByLabelText('Color preview');
    expect(preview).toBeInTheDocument();
  });
});
