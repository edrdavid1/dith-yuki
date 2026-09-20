import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../shared/ui/Notification.module.css';
import { bind } from '../shared/ui/cn';
import type { SoftDiscardInfo } from '../shared/ipc/recovery';

const cn = bind(styles);

const RESTORE_MS = 12_000;

export interface DiscardRestoreToastProps {
  info: SoftDiscardInfo | null;
  onRestore: (info: SoftDiscardInfo) => void;
  onDismiss: () => void;
}

/** Soft-discard toast after tab × → Don’t Save (spec Phase 4). */
export default function DiscardRestoreToast({
  info,
  onRestore,
  onDismiss,
}: DiscardRestoreToastProps) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!info) {
      setVisible(false);
      return;
    }
    setVisible(true);
    const timer = setTimeout(() => {
      setVisible(false);
      onDismiss();
    }, RESTORE_MS);
    return () => clearTimeout(timer);
  }, [info, onDismiss]);

  const handleRestore = useCallback(() => {
    if (!info) return;
    setVisible(false);
    onRestore(info);
  }, [info, onRestore]);

  if (!visible || !info) return null;

  return createPortal(
    <div
      className={cn('notification', 'notification-success')}
      data-testid="discard-restore-toast"
      role="status"
    >
      <span className={cn('notification-text')}>
        Discarded {info.display_name}
      </span>
      <button type="button" className={cn('notification-close')} onClick={handleRestore}>
        Restore
      </button>
      <button
        type="button"
        className={cn('notification-close')}
        onClick={() => {
          setVisible(false);
          onDismiss();
        }}
        aria-label="Dismiss"
      >
        ×
      </button>
    </div>,
    document.body
  );
}
