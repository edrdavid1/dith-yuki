import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../../shared/ui/Notification.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

const DEFAULT_DURATION_MS = 5000;

export interface NotificationProps {
  message: string | null;
  /** Optional underlined action (e.g. Restore). */
  actionLabel?: string;
  onAction?: () => void;
  onDismiss?: () => void;
  /**
   * Auto-dismiss after ms. `null` keeps the toast until `message` clears
   * (busy / in-flight states).
   */
  duration?: number | null;
  /** Pulse indicator; toast is non-interactive (no close / action). */
  busy?: boolean;
  /** When false, hide the × (defaults to !busy). */
  dismissible?: boolean;
  testId?: string;
}

/**
 * Shell toast used for errors, success, busy, and action prompts (Restore).
 * Matches the soft-discard Restore chrome: fixed bottom bar, portal to body.
 */
function Notification({
  message,
  actionLabel,
  onAction,
  onDismiss,
  duration = DEFAULT_DURATION_MS,
  busy = false,
  dismissible,
  testId,
}: NotificationProps) {
  const [visible, setVisible] = useState(false);
  const canDismiss = dismissible ?? !busy;

  useEffect(() => {
    if (!message) {
      setVisible(false);
      return;
    }
    setVisible(true);
    if (duration == null) return;
    const timer = setTimeout(() => {
      setVisible(false);
      onDismiss?.();
    }, duration);
    return () => clearTimeout(timer);
  }, [message, duration, onDismiss]);

  if (!visible || !message) return null;

  const dismiss = () => {
    setVisible(false);
    onDismiss?.();
  };

  return createPortal(
    <div
      className={cn('notification', busy && 'notification-busy')}
      role="status"
      aria-live="polite"
      aria-busy={busy || undefined}
      data-testid={testId}
    >
      {busy ? <span className={cn('notification-busy-dot')} aria-hidden /> : null}
      <span className={cn('notification-text')}>{message}</span>
      {!busy && actionLabel && onAction ? (
        <button
          type="button"
          className={cn('notification-action')}
          onClick={() => {
            setVisible(false);
            onAction();
          }}
        >
          {actionLabel}
        </button>
      ) : null}
      {canDismiss ? (
        <button
          type="button"
          className={cn('notification-close')}
          onClick={dismiss}
          aria-label="Dismiss"
        >
          ×
        </button>
      ) : null}
    </div>,
    document.body
  );
}

/** Busy / in-flight toast (open, save, …). Stays until `message` is cleared. */
export function BusyNotification({
  message,
  testId = 'busy-notification',
}: {
  message: string | null;
  testId?: string;
}) {
  return (
    <Notification
      message={message}
      busy
      duration={null}
      testId={testId}
    />
  );
}

export default Notification;
