import Notification from './common/Notification';
import type { SoftDiscardInfo } from '../shared/ipc/recovery';

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
  return (
    <Notification
      message={info ? `Discarded ${info.display_name}` : null}
      actionLabel="Restore"
      onAction={() => {
        if (info) onRestore(info);
      }}
      onDismiss={onDismiss}
      duration={RESTORE_MS}
      testId="discard-restore-toast"
    />
  );
}
