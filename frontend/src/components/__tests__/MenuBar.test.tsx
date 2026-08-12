import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import MenuBar from '../MenuBar';

function renderMenuBar(overrides?: Partial<React.ComponentProps<typeof MenuBar>>) {
  const defaults = {
    hasDocument: true,
    onOpenImage: vi.fn(),
    onSaveImage: vi.fn(),
    onOpenColorLab: vi.fn(),
    onOpenPreferences: vi.fn(),
  };
  const props = { ...defaults, ...overrides };
  return { ...render(<MenuBar {...props} />), props };
}

describe('MenuBar', () => {
  it('renders all menu items in order', () => {
    renderMenuBar();
    const buttons = screen.getAllByRole('menuitem');
    // Top-level menu items
    const labels = buttons.map((b) => b.textContent);
    expect(labels).toContain('File');
    expect(labels).toContain('Edit');
    expect(labels).toContain('Presets');
    expect(labels).toContain('Color Lab');
    expect(labels).toContain('Preferences');
    expect(labels).toContain('Help');
  });

  it('opens File dropdown on click with correct items', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    expect(screen.getByText('Save/Export')).toBeInTheDocument();
  });

  it('opens Edit dropdown with disabled Undo/Redo', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('Edit'));
    const undo = screen.getByText('Undo');
    const redo = screen.getByText('Redo');
    expect(undo).toBeDisabled();
    expect(redo).toBeDisabled();
  });

  it('closes dropdown when clicking the same menu item again', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.click(screen.getByText('File'));
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
  });

  it('switches dropdown on hover when one is already open', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();

    // Hover over Edit
    fireEvent.mouseEnter(screen.getByText('Edit'));
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
    expect(screen.getByText('Undo')).toBeInTheDocument();
  });

  it('does not open dropdown on hover when none is open', () => {
    renderMenuBar();
    fireEvent.mouseEnter(screen.getByText('File'));
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
  });

  it('Color Lab click calls onOpenColorLab directly (no dropdown)', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('Color Lab'));
    expect(props.onOpenColorLab).toHaveBeenCalledTimes(1);
    // No dropdown items should appear
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('Preferences click calls onOpenPreferences directly (no dropdown)', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('Preferences'));
    expect(props.onOpenPreferences).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('closes dropdown on Escape key', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
  });

  it('closes dropdown on click outside', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
  });

  it('calls onOpenImage when File > Open Image is clicked', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    fireEvent.click(screen.getByText('Open Image'));
    expect(props.onOpenImage).toHaveBeenCalledTimes(1);
  });

  it('calls onSaveImage when File > Save/Export is clicked', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    fireEvent.click(screen.getByText('Save/Export'));
    expect(props.onSaveImage).toHaveBeenCalledTimes(1);
  });

  it('disables Save/Export when hasDocument is false', () => {
    renderMenuBar({ hasDocument: false });
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Save/Export')).toBeDisabled();
  });

  it('hovering Color Lab when dropdown is open closes the dropdown', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByText('Color Lab'));
    // Dropdown should be closed since Color Lab has no dropdown
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('hovering Preferences when dropdown is open closes the dropdown', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByText('Preferences'));
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
