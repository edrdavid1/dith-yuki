import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import NewProjectDialog from '../NewProjectDialog';

function renderDialog(
  overrides?: Partial<React.ComponentProps<typeof NewProjectDialog>>
) {
  const defaults = {
    isOpen: true,
    onCreate: vi.fn(),
    onClose: vi.fn(),
  };
  const props = { ...defaults, ...overrides };
  return { ...render(<NewProjectDialog {...props} />), props };
}

describe('NewProjectDialog', () => {
  it('renders nothing when isOpen is false', () => {
    const { container } = render(
      <NewProjectDialog isOpen={false} onCreate={vi.fn()} onClose={vi.fn()} />
    );
    expect(container.innerHTML).toBe('');
  });

  it('does not submit width 0', () => {
    const { props } = renderDialog();
    fireEvent.change(screen.getByLabelText('Width'), { target: { value: '0' } });
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
    fireEvent.submit(screen.getByRole('button', { name: 'Create' }).closest('form')!);
    expect(props.onCreate).not.toHaveBeenCalled();
  });

  it('does not submit a negative height', () => {
    const { props } = renderDialog();
    fireEvent.change(screen.getByLabelText('Height'), { target: { value: '-10' } });
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
    fireEvent.submit(screen.getByRole('button', { name: 'Create' }).closest('form')!);
    expect(props.onCreate).not.toHaveBeenCalled();
  });

  it('does not submit a size above 8192', () => {
    const { props } = renderDialog();
    fireEvent.change(screen.getByLabelText('Width'), { target: { value: '9000' } });
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
    fireEvent.submit(screen.getByRole('button', { name: 'Create' }).closest('form')!);
    expect(props.onCreate).not.toHaveBeenCalled();
  });

  it('submits defaults 1920×1080 transparent', () => {
    const { props } = renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(props.onCreate).toHaveBeenCalledWith({
      width: 1920,
      height: 1080,
      background: 'transparent',
    });
  });
});
