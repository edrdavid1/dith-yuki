import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import SwatchGrid from '../SwatchGrid';

// Mock the IPC commands
vi.mock('../../ipc/commands', () => ({
  addColorToPalette: vi.fn(),
  updatePaletteColor: vi.fn(),
  removePaletteColor: vi.fn(),
  reorderPaletteColor: vi.fn(),
}));

import {
  addColorToPalette,
  updatePaletteColor,
  removePaletteColor,
  reorderPaletteColor,
} from '../../ipc/commands';

const mockedAdd = vi.mocked(addColorToPalette);
const mockedUpdate = vi.mocked(updatePaletteColor);
const mockedRemove = vi.mocked(removePaletteColor);
const mockedReorder = vi.mocked(reorderPaletteColor);

function renderGrid(overrides?: Partial<React.ComponentProps<typeof SwatchGrid>>) {
  const defaults = {
    paletteId: 1,
    colors: ['FF0000', '00FF00', '0000FF'],
    onColorAdded: vi.fn(),
    onColorUpdated: vi.fn(),
    onColorRemoved: vi.fn(),
    onColorReordered: vi.fn(),
  };
  const props = { ...defaults, ...overrides };
  return { ...render(<SwatchGrid {...props} />), props };
}

describe('SwatchGrid', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedAdd.mockResolvedValue({ id: 1, name: 'Test', colors: [], hex_colors: [], color_count: 0 });
    mockedUpdate.mockResolvedValue({ id: 1, name: 'Test', colors: [], hex_colors: [], color_count: 0 });
    mockedRemove.mockResolvedValue({ id: 1, name: 'Test', colors: [], hex_colors: [], color_count: 0 });
    mockedReorder.mockResolvedValue({ id: 1, name: 'Test', colors: [], hex_colors: [], color_count: 0 });
  });

  it('renders correct number of swatches', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    expect(swatches).toHaveLength(3);
  });

  it('renders swatches with correct background colors', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    expect(swatches[0]).toHaveStyle({ backgroundColor: '#FF0000' });
    expect(swatches[1]).toHaveStyle({ backgroundColor: '#00FF00' });
    expect(swatches[2]).toHaveStyle({ backgroundColor: '#0000FF' });
  });

  it('shows hex code as tooltip on each swatch', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    expect(swatches[0]).toHaveAttribute('title', 'FF0000');
    expect(swatches[1]).toHaveAttribute('title', '00FF00');
    expect(swatches[2]).toHaveAttribute('title', '0000FF');
  });

  it('renders "+" add button', () => {
    renderGrid();
    expect(screen.getByLabelText('Add color')).toBeInTheDocument();
  });

  it('renders "−" remove button', () => {
    renderGrid();
    expect(screen.getByLabelText('Remove selected color')).toBeInTheDocument();
  });

  it('selects a swatch on click with visible highlight', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.click(swatches[1]);
    expect(swatches[1]).toHaveAttribute('aria-pressed', 'true');
    expect(swatches[0]).toHaveAttribute('aria-pressed', 'false');
  });

  it('deselects previous swatch when clicking a new one', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.click(swatches[0]);
    expect(swatches[0]).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(swatches[2]);
    expect(swatches[0]).toHaveAttribute('aria-pressed', 'false');
    expect(swatches[2]).toHaveAttribute('aria-pressed', 'true');
  });

  it('opens ColorPicker on double-click with the swatch color', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.doubleClick(swatches[0]);
    // ColorPicker should appear as a dialog
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    // Should have the color pre-filled
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    expect(input.value).toBe('FF0000');
  });

  it('opens ColorPicker in add mode when "+" is clicked', () => {
    renderGrid();
    fireEvent.click(screen.getByLabelText('Add color'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    // Default color FFFFFF in add mode
    const input = screen.getByLabelText('Hex color value') as HTMLInputElement;
    expect(input.value).toBe('FFFFFF');
  });

  it('calls addColorToPalette and onColorAdded on add confirm', async () => {
    const { props } = renderGrid();
    fireEvent.click(screen.getByLabelText('Add color'));
    fireEvent.click(screen.getByText('Confirm'));
    await waitFor(() => {
      expect(mockedAdd).toHaveBeenCalledWith(1, 'FFFFFF');
      expect(props.onColorAdded).toHaveBeenCalledTimes(1);
    });
  });

  it('calls updatePaletteColor and onColorUpdated on edit confirm', async () => {
    const { props } = renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.doubleClick(swatches[1]);
    fireEvent.click(screen.getByText('Confirm'));
    await waitFor(() => {
      expect(mockedUpdate).toHaveBeenCalledWith(1, 1, '00FF00');
      expect(props.onColorUpdated).toHaveBeenCalledTimes(1);
    });
  });

  it('calls removePaletteColor and deselects on remove', async () => {
    const { props } = renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.click(swatches[0]);
    fireEvent.click(screen.getByLabelText('Remove selected color'));
    await waitFor(() => {
      expect(mockedRemove).toHaveBeenCalledWith(1, 0);
      expect(props.onColorRemoved).toHaveBeenCalledTimes(1);
    });
  });

  it('remove button is disabled when no swatch is selected', () => {
    renderGrid();
    const removeBtn = screen.getByLabelText('Remove selected color');
    expect(removeBtn).toBeDisabled();
  });

  it('shows error message when IPC command fails', async () => {
    mockedRemove.mockRejectedValueOnce('Palette not found');
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    fireEvent.click(swatches[0]);
    fireEvent.click(screen.getByLabelText('Remove selected color'));
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Palette not found');
    });
  });

  it('shows only "+" and disabled "−" when palette has 0 colors', () => {
    renderGrid({ colors: [] });
    expect(screen.getByLabelText('Add color')).toBeInTheDocument();
    expect(screen.getByLabelText('Remove selected color')).toBeDisabled();
    expect(screen.queryAllByRole('button', { name: /Color swatch/ })).toHaveLength(0);
  });

  it('closes ColorPicker on cancel without calling IPC', () => {
    renderGrid();
    fireEvent.click(screen.getByLabelText('Add color'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Cancel'));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(mockedAdd).not.toHaveBeenCalled();
  });

  it('swatches are draggable', () => {
    renderGrid();
    const swatches = screen.getAllByRole('button', { name: /Color swatch/ });
    expect(swatches[0]).toHaveAttribute('draggable', 'true');
  });
});
