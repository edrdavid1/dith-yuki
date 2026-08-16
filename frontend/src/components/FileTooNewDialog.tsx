import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import type { UpdateCheckResult } from '../shared/ipc/updates';

const cn = bind(styles);

export interface FileTooNewDialogProps {
  isOpen: boolean;
  message: string;
  checking: boolean;
  checkResult: UpdateCheckResult | null;
  onClose: () => void;
  onCheck: () => void;
  onInstall: () => void;
}

export default function FileTooNewDialog({
  isOpen,
  message,
  checking,
  checkResult,
  onClose,
  onCheck,
  onInstall,
}: FileTooNewDialogProps) {
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  const alreadyLatest = checkResult?.status === 'none';
  const available = checkResult?.status === 'available' ? checkResult.update : null;
  const checkError = checkResult?.status === 'error' ? checkResult.message : null;

  return createPortal(
    <div className={cn('new-project-overlay')} onClick={handleOverlayClick}>
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="file-too-new-title"
      >
        <DialogTitlebar title="Update required" titleId="file-too-new-title" onClose={onClose} />
        <div className={cn('new-project-body')}>
          <p className={cn('new-project-field')}>{message}</p>
          {alreadyLatest ? (
            <p className={cn('new-project-field')}>
              This app is already up to date. The file is from a newer or private build.
            </p>
          ) : null}
          {available ? (
            <p className={cn('new-project-field')}>Version {available.version} is available.</p>
          ) : null}
          {checkError ? <p className={cn('new-project-error')}>{checkError}</p> : null}
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Close
            </button>
            {available ? (
              <button
                type="button"
                className={cn('new-project-btn', 'new-project-btn-primary')}
                onClick={onInstall}
              >
                Install and Restart
              </button>
            ) : (
              <button
                type="button"
                className={cn('new-project-btn', 'new-project-btn-primary')}
                onClick={onCheck}
                disabled={checking}
              >
                {checking ? 'Checking…' : 'Check for Updates…'}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
