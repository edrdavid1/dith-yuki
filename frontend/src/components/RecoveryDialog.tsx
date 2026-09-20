import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import type { JournalMeta } from '../shared/ipc/recovery';

const cn = bind(styles);

export interface RecoveryDialogProps {
  journals: JournalMeta[];
  busy?: boolean;
  onRecoverAll: () => void;
  onDiscardAll: () => void;
  onSkip: () => void;
}

export default function RecoveryDialog({
  journals,
  busy = false,
  onRecoverAll,
  onDiscardAll,
  onSkip,
}: RecoveryDialogProps) {
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget && !busy) onSkip();
    },
    [busy, onSkip]
  );

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="recovery-dialog-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="recovery-dialog-title"
      >
        <DialogTitlebar
          title="Recover unsaved work?"
          titleId="recovery-dialog-title"
          onClose={onSkip}
        />
        <div className={cn('new-project-body')} style={{ padding: '14px 12px 12px' }}>
          <p className={cn('new-project-field')}>
            Unsaved recovery found ({journals.length}{' '}
            {journals.length === 1 ? 'document' : 'documents'}). The previous session may have
            crashed or been force-quit.
          </p>
          <ul
            className={cn('new-project-field')}
            style={{ listStyle: 'none', margin: 0, padding: 0, gap: 6 }}
            data-testid="recovery-doc-list"
          >
            {journals.map((j) => (
              <li key={j.recovery_id} className={cn('new-project-radio')}>
                {j.display_name}
                {j.project_path ? (
                  <span style={{ opacity: 0.6, marginLeft: 6 }}>({j.project_path})</span>
                ) : null}
              </li>
            ))}
          </ul>
          <div className={cn('new-project-footer')}>
            <button
              type="button"
              className={cn('new-project-btn')}
              onClick={onSkip}
              disabled={busy}
            >
              Open originals only
            </button>
            <button
              type="button"
              className={cn('new-project-btn')}
              onClick={onDiscardAll}
              disabled={busy}
            >
              Discard journals
            </button>
            <button
              type="button"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              onClick={onRecoverAll}
              disabled={busy}
            >
              Recover all
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
