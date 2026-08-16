import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export interface UpdateAvailableDialogProps {
  isOpen: boolean;
  version: string;
  notes: string;
  phase: 'prompt' | 'downloading';
  downloaded: number;
  contentLength: number | null;
  error: string | null;
  onLater: () => void;
  onInstall: () => void;
  onCancelDownload: () => void;
}

export default function UpdateAvailableDialog({
  isOpen,
  version,
  notes,
  phase,
  downloaded,
  contentLength,
  error,
  onLater,
  onInstall,
  onCancelDownload,
}: UpdateAvailableDialogProps) {
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target !== e.currentTarget) return;
      if (phase === 'downloading') onCancelDownload();
      else onLater();
    },
    [onCancelDownload, onLater, phase]
  );

  if (!isOpen) return null;

  const determinate =
    contentLength != null && contentLength > 0
      ? Math.min(100, Math.round((downloaded / contentLength) * 100))
      : null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="update-available-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-available-title"
      >
        <DialogTitlebar
          title="Update available"
          titleId="update-available-title"
          onClose={phase === 'downloading' ? onCancelDownload : onLater}
        />
        <div className={cn('new-project-body')}>
          <p className={cn('new-project-field')}>Version {version} is available.</p>
          {notes ? <p className={cn('new-project-notes')}>{notes}</p> : null}
          {phase === 'downloading' ? (
            <div
              className={cn('new-project-progress-track')}
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={determinate ?? undefined}
            >
              <div
                className={cn('new-project-progress-fill')}
                style={{ width: determinate != null ? `${determinate}%` : '40%' }}
              />
            </div>
          ) : null}
          {error ? <p className={cn('new-project-error')}>{error}</p> : null}
          <div className={cn('new-project-footer')}>
            {phase === 'downloading' ? (
              <button type="button" className={cn('new-project-btn')} onClick={onCancelDownload}>
                Cancel
              </button>
            ) : (
              <>
                <button type="button" className={cn('new-project-btn')} onClick={onLater}>
                  Later
                </button>
                <button
                  type="button"
                  className={cn('new-project-btn', 'new-project-btn-primary')}
                  onClick={onInstall}
                >
                  Install and Restart
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
