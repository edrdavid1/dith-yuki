import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import type { JournalMeta, RosterEntry } from '../shared/ipc/recovery';

const cn = bind(styles);

export interface RecoveryDialogProps {
  journals: JournalMeta[];
  /** Last-session open set (may be shown when journals are empty). */
  rosterDocs?: RosterEntry[];
  busy?: boolean;
  onRecoverAll: () => void;
  onDiscardAll: () => void;
  onReopenRoster?: () => void;
  onSkip: () => void;
}

export default function RecoveryDialog({
  journals,
  rosterDocs = [],
  busy = false,
  onRecoverAll,
  onDiscardAll,
  onReopenRoster,
  onSkip,
}: RecoveryDialogProps) {
  const hasJournals = journals.length > 0;
  const hasRoster = rosterDocs.length > 0;
  const title = hasJournals ? 'Recover unsaved work?' : 'Reopen previous session?';

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
        <DialogTitlebar title={title} titleId="recovery-dialog-title" onClose={onSkip} />
        <div className={cn('new-project-body')} style={{ padding: '14px 12px 12px' }}>
          {hasJournals ? (
            <>
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
                      <span className={cn('new-project-path')}>({j.project_path})</span>
                    ) : null}
                  </li>
                ))}
              </ul>
            </>
          ) : null}

          {hasRoster && !hasJournals ? (
            <>
              <p className={cn('new-project-field')}>
                Previous session had {rosterDocs.length}{' '}
                {rosterDocs.length === 1 ? 'document' : 'documents'} open. Reopen them?
              </p>
              <ul
                className={cn('new-project-field')}
                style={{ listStyle: 'none', margin: 0, padding: 0, gap: 6 }}
                data-testid="recovery-roster-list"
              >
                {rosterDocs.map((e) => (
                  <li key={e.recovery_id} className={cn('new-project-radio')}>
                    {e.display_name}
                    {e.project_path ? (
                      <span className={cn('new-project-path')}>({e.project_path})</span>
                    ) : null}
                  </li>
                ))}
              </ul>
            </>
          ) : null}

          {hasRoster && hasJournals ? (
            <p className={cn('new-project-field')} style={{ opacity: 0.75 }}>
              Also had {rosterDocs.length} open tab
              {rosterDocs.length === 1 ? '' : 's'} last session
              {onReopenRoster ? ' — you can reopen saved files after recover.' : '.'}
            </p>
          ) : null}

          <div className={cn('new-project-footer')}>
            <button
              type="button"
              className={cn('new-project-btn')}
              onClick={onSkip}
              disabled={busy}
            >
              {hasJournals ? 'Open originals only' : 'Skip'}
            </button>
            {hasJournals ? (
              <button
                type="button"
                className={cn('new-project-btn')}
                onClick={onDiscardAll}
                disabled={busy}
              >
                Discard journals
              </button>
            ) : null}
            {!hasJournals && hasRoster && onReopenRoster ? (
              <button
                type="button"
                className={cn('new-project-btn', 'new-project-btn-primary')}
                onClick={onReopenRoster}
                disabled={busy}
              >
                {busy ? 'Reopening…' : 'Reopen'}
              </button>
            ) : null}
            {hasJournals ? (
              <button
                type="button"
                className={cn('new-project-btn', 'new-project-btn-primary')}
                onClick={onRecoverAll}
                disabled={busy}
              >
                {busy ? 'Recovering…' : 'Recover all'}
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
