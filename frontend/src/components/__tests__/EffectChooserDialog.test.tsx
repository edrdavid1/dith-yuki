import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import EffectChooserDialog from '../EffectChooserDialog';

function renderDialog(overrides?: Partial<React.ComponentProps<typeof EffectChooserDialog>>) {
  const defaults = {
    isOpen: true,
    onSelect: vi.fn(),
    onClose: vi.fn(),
  };
  const props = { ...defaults, ...overrides };
  return { ...render(<EffectChooserDialog {...props} />), props };
}

describe('EffectChooserDialog', () => {
  it('renders nothing when isOpen=false', () => {
    const { container } = render(
      <EffectChooserDialog isOpen={false} onSelect={vi.fn()} onClose={vi.fn()} />
    );
    expect(container.innerHTML).toBe('');
  });

  it('renders 7 effect items when isOpen=true', () => {
    renderDialog();
    const items = screen.getAllByRole('option');
    expect(items).toHaveLength(7);
    expect(screen.getByText('Dithering')).toBeInTheDocument();
    expect(screen.getByText('Glitching')).toBeInTheDocument();
    expect(screen.getByText('Curves')).toBeInTheDocument();
    expect(screen.getByText('RGB Channels')).toBeInTheDocument();
    expect(screen.getByText('Glow')).toBeInTheDocument();
    expect(screen.getByText('CRT')).toBeInTheDocument();
    expect(screen.getByText('Adjust')).toBeInTheDocument();
  });

  it('calls onSelect with "Dithering" when first item is clicked', () => {
    const { props } = renderDialog();
    fireEvent.click(screen.getByText('Dithering'));
    expect(props.onSelect).toHaveBeenCalledWith('Dithering');
  });

  it('calls onSelect with "Glitching" when second item is clicked', () => {
    const { props } = renderDialog();
    fireEvent.click(screen.getByText('Glitching'));
    expect(props.onSelect).toHaveBeenCalledWith('Glitching');
  });

  it('calls onSelect with "Curves" when third item is clicked', () => {
    const { props } = renderDialog();
    fireEvent.click(screen.getByText('Curves'));
    expect(props.onSelect).toHaveBeenCalledWith('Curves');
  });

  it('calls onSelect with "RGBChannels" when fourth item is clicked', () => {
    const { props } = renderDialog();
    fireEvent.click(screen.getByText('RGB Channels'));
    expect(props.onSelect).toHaveBeenCalledWith('RGBChannels');
  });

  it('calls onClose on Escape key', () => {
    const { props } = renderDialog();
    const dialog = screen.getByRole('dialog');
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose on overlay click', () => {
    const { props } = renderDialog();
    const overlay = screen.getByTestId('effect-chooser-overlay');
    fireEvent.click(overlay);
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it('does not call onClose when clicking inside the dialog', () => {
    const { props } = renderDialog();
    const dialog = screen.getByRole('dialog');
    fireEvent.click(dialog);
    expect(props.onClose).not.toHaveBeenCalled();
  });

  it('calls onClose when close button is clicked', () => {
    const { props } = renderDialog();
    const closeBtn = screen.getByLabelText('Close');
    fireEvent.click(closeBtn);
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it('arrow down moves focus to the next item', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    const items = screen.getAllByRole('option');

    // Initially first item is focused
    expect(items[0]).toHaveAttribute('aria-selected', 'true');

    // Press arrow down
    fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    expect(items[1]).toHaveAttribute('aria-selected', 'true');
    expect(items[0]).toHaveAttribute('aria-selected', 'false');
  });

  it('arrow up moves focus to previous item', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    const items = screen.getAllByRole('option');

    // Move down first
    fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    expect(items[2]).toHaveAttribute('aria-selected', 'true');

    // Move up
    fireEvent.keyDown(dialog, { key: 'ArrowUp' });
    expect(items[1]).toHaveAttribute('aria-selected', 'true');
  });

  it('Enter confirms current selection', () => {
    const { props } = renderDialog();
    const dialog = screen.getByRole('dialog');

    // Move down to Glitching
    fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    // Press Enter
    fireEvent.keyDown(dialog, { key: 'Enter' });
    expect(props.onSelect).toHaveBeenCalledWith('Glitching');
  });

  it('arrow down does not go past last item', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    const items = screen.getAllByRole('option');

    // Press arrow down 20 times (more than the list)
    for (let i = 0; i < 20; i++) {
      fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    }
    // Should stay on last item
    expect(items[items.length - 1]).toHaveAttribute('aria-selected', 'true');
  });

  it('arrow up does not go before first item', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    const items = screen.getAllByRole('option');

    // Press arrow up at beginning
    fireEvent.keyDown(dialog, { key: 'ArrowUp' });
    expect(items[0]).toHaveAttribute('aria-selected', 'true');
  });

  it('renders with dialog title "Effect"', () => {
    renderDialog();
    expect(screen.getByText('Effect')).toBeInTheDocument();
  });
});
