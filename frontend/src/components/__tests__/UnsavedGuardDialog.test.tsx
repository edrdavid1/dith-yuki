import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import UnsavedGuardDialog from '../UnsavedGuardDialog';

describe('UnsavedGuardDialog', () => {
  it('renders three actions', () => {
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
