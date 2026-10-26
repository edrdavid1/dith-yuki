import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import Notification, { BusyNotification } from '../common/Notification';
import DiscardRestoreToast from '../DiscardRestoreToast';
import type { SoftDiscardInfo } from '../../shared/ipc/recovery';

const softDiscard: SoftDiscardInfo = {
  recovery_id: 'rec-1',
  display_name: 'photo.png',
};

describe('Notification (shared shell)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders message with dismiss control via portal', () => {
    const onDismiss = vi.fn();
    render(<Notification message="Saved" onDismiss={onDismiss} />);
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
    fireEvent.click(screen.getByLabelText('Dismiss'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('auto-dismisses after duration', () => {
    const onDismiss = vi.fn();
    render(<Notification message="Hello" duration={1000} onDismiss={onDismiss} />);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('shows action button when provided', () => {
    const onAction = vi.fn();
    render(
      <Notification
        message="Discarded photo.png"
        actionLabel="Restore"
        onAction={onAction}
        duration={null}
      />
    );
    fireEvent.click(screen.getByText('Restore'));
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it('busy toast has no dismiss control and stays until cleared', () => {
    const { rerender } = render(<BusyNotification message="Opening…" />);
    const toast = screen.getByTestId('busy-notification');
    expect(toast).toHaveTextContent('Opening…');
    expect(toast).toHaveAttribute('aria-busy', 'true');
    expect(screen.queryByLabelText('Dismiss')).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByTestId('busy-notification')).toBeInTheDocument();
    rerender(<BusyNotification message={null} />);
    expect(screen.queryByTestId('busy-notification')).not.toBeInTheDocument();
  });

  it('DiscardRestoreToast uses the shared shell', () => {
    const onRestore = vi.fn();
    const onDismiss = vi.fn();
    render(
      <DiscardRestoreToast info={softDiscard} onRestore={onRestore} onDismiss={onDismiss} />
    );
    expect(screen.getByTestId('discard-restore-toast')).toHaveTextContent(
      'Discarded photo.png'
    );
    fireEvent.click(screen.getByText('Restore'));
    expect(onRestore).toHaveBeenCalledWith(softDiscard);
  });
});
