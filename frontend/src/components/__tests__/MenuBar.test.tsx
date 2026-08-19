import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import MenuBar from '../MenuBar';

function renderMenuBar(overrides?: Partial<React.ComponentProps<typeof MenuBar>>) {
  const defaults = {
    hasDocument: true,
    onNewProject: vi.fn(),
    onOpenImage: vi.fn(),
    onImportImageLayer: vi.fn(),
    onSaveImage: vi.fn(),
    onOpenProject: vi.fn(),
    onOpenRecent: vi.fn(),
    onSaveProject: vi.fn(),
    onSaveProjectAs: vi.fn(),
    onExportPattern: vi.fn(),
    onImportPattern: vi.fn(),
    onOpenColorLab: vi.fn(),
    onOpenPreferences: vi.fn(),
    onOpenHelp: vi.fn(),
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
    expect(screen.getByText('New Project…')).toBeInTheDocument();
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    expect(screen.getByText('Import Image as Layer…')).toBeInTheDocument();
    expect(screen.getByText('Open Project…')).toBeInTheDocument();
    expect(screen.getByText('Save Project')).toBeInTheDocument();
    expect(screen.getByText('Save Project As…')).toBeInTheDocument();
    expect(screen.getByText('Save/Export')).toBeInTheDocument();
  });

  it('opens Presets dropdown with pattern export/import', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('Presets'));
    expect(screen.getByText('Export Pattern…')).toBeInTheDocument();
    expect(screen.getByText('Import Pattern…')).toBeInTheDocument();
  });

  it('opens Edit dropdown with disabled Undo/Redo by default', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('Edit'));
    const undo = screen.getByRole('menuitem', { name: /Undo/i });
    const redo = screen.getByRole('menuitem', { name: /Redo/i });
    expect(undo).toBeDisabled();
    expect(redo).toBeDisabled();
  });

  it('enables Undo/Redo from flags and invokes callbacks', () => {
    const onUndo = vi.fn();
    const onRedo = vi.fn();
    renderMenuBar({ canUndo: true, canRedo: true, onUndo, onRedo });
    fireEvent.click(screen.getByText('Edit'));
    const undo = screen.getByRole('menuitem', { name: /Undo/i });
    const redo = screen.getByRole('menuitem', { name: /Redo/i });
    expect(undo).toBeEnabled();
    expect(redo).toBeEnabled();
    fireEvent.click(undo);
    expect(onUndo).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText('Edit'));
    fireEvent.click(screen.getByRole('menuitem', { name: /Redo/i }));
    expect(onRedo).toHaveBeenCalledTimes(1);
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

  it('calls onImportImageLayer when File > Import Image as Layer is clicked', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    fireEvent.click(screen.getByText('Import Image as Layer…'));
    expect(props.onImportImageLayer).toHaveBeenCalledTimes(1);
  });

  it('calls onExportPattern when Presets > Export Pattern is clicked', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('Presets'));
    fireEvent.click(screen.getByText('Export Pattern…'));
    expect(props.onExportPattern).toHaveBeenCalledTimes(1);
  });

  it('calls onImportPattern when Presets > Import Pattern is clicked', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('Presets'));
    fireEvent.click(screen.getByText('Import Pattern…'));
    expect(props.onImportPattern).toHaveBeenCalledTimes(1);
  });

  it('disables Save/Export when hasDocument is false', () => {
    renderMenuBar({ hasDocument: false });
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByRole('menuitem', { name: /Save\/Export/ })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: /^Save Project\b/ })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: /Save Project As/ })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: /Import Image as Layer/ })).toBeDisabled();
  });

  it('disables pattern actions in Presets when hasDocument is false', () => {
    renderMenuBar({ hasDocument: false });
    fireEvent.click(screen.getByText('Presets'));
    expect(screen.getByText('Export Pattern…')).toBeDisabled();
    expect(screen.getByText('Import Pattern…')).toBeDisabled();
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

  it('shows New Project even when a document is open', () => {
    const { props } = renderMenuBar({ hasDocument: true });
    fireEvent.click(screen.getByText('File'));
    const item = screen.getByText('New Project…');
    expect(item).toBeEnabled();
    fireEvent.click(item);
    expect(props.onNewProject).toHaveBeenCalledTimes(1);
  });

  it('hides Open Recent when the recent list is empty', () => {
    renderMenuBar({ recentEntries: [] });
    fireEvent.click(screen.getByText('File'));
    expect(screen.queryByText('Open Recent')).not.toBeInTheDocument();
  });

  it('opens a recent entry from Open Recent', () => {
    const { props } = renderMenuBar({
      recentEntries: [
        {
          path: '/tmp/proj.dyproj',
          kind: 'project',
          display_name: 'proj.dyproj',
          opened_at: '2026-08-13T00:00:00.000Z',
        },
      ],
    });
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Recent')).toBeInTheDocument();
    fireEvent.click(screen.getByText('proj.dyproj'));
    expect(props.onOpenRecent).toHaveBeenCalledWith(
      expect.objectContaining({ path: '/tmp/proj.dyproj', kind: 'project' })
    );
  });

  it('Help click calls onOpenHelp directly (no dropdown)', () => {
    const { props } = renderMenuBar();
    fireEvent.click(screen.getByText('Help'));
    expect(props.onOpenHelp).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('hovering Help when dropdown is open closes the dropdown', () => {
    renderMenuBar();
    fireEvent.click(screen.getByText('File'));
    expect(screen.getByText('Open Image')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByText('Help'));
    expect(screen.queryByText('Open Image')).not.toBeInTheDocument();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
