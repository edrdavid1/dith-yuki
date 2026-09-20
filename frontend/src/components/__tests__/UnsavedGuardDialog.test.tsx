import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import UnsavedGuardDialog from '../UnsavedGuardDialog';

describe('UnsavedGuardDialog', () => {
  it('renders three actions in single-doc mode', () => {
    const onSave = vi.fn();
    const onDiscard = vi.fn();
    const onCancel = vi.fn();
    render(
      <UnsavedGuardDialog
        isOpen
        basename="qa.dyproj"
        onSave={onSave}
        onDiscard={onDiscard}
        onCancel={onCancel}
      />
    );
    expect(screen.getByText(/qa.dyproj/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Don.t Save/ }));
    expect(onDiscard).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onSave).toHaveBeenCalled();
  });

  it('renders a checklist in multi-doc mode', () => {
    const onToggleSelected = vi.fn();
    const onSave = vi.fn();
    render(
      <UnsavedGuardDialog
        isOpen
        documents={[
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ]}
        selectedIds={[1, 2]}
        onToggleSelected={onToggleSelected}
        onSave={onSave}
        onDiscard={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(screen.getByTestId('unsaved-guard-doc-list')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Discard All' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save Selected & Quit' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: 'a.dyproj' }));
    expect(onToggleSelected).toHaveBeenCalledWith(1);
  });

  it('disables Save Selected when nothing is checked', () => {
    render(
      <UnsavedGuardDialog
        isOpen
        documents={[
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ]}
        selectedIds={[]}
        onSave={vi.fn()}
        onDiscard={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(screen.getByRole('button', { name: 'Save Selected & Quit' })).toBeDisabled();
  });

  it('renders nothing when closed', () => {
    const { container } = render(
      <UnsavedGuardDialog
        isOpen={false}
        basename="x"
        onSave={vi.fn()}
        onDiscard={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(container.innerHTML).toBe('');
  });
});
