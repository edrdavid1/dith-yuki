import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export interface HelpDialogProps {
  isOpen: boolean;
  version: string;
  checking?: boolean;
  onClose: () => void;
  onCheckForUpdates?: () => void;
}

export default function HelpDialog({
  isOpen,
  version,
  checking = false,
  onClose,
  onCheckForUpdates,
}: HelpDialogProps) {
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [onClose]
  );

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="help-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="help-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Help" titleId="help-title" onClose={onClose} />
        <div className={cn('new-project-body')}>
          <p className={cn('new-project-field')}>About</p>
          <p className={cn('new-project-field')}>
            Dither Engine {version || '…'} — pixel art / dithering workspace.
          </p>
          <div className={cn('new-project-footer')}>
            {onCheckForUpdates && (
              <button
                type="button"
                className={cn('new-project-btn', 'new-project-btn-primary')}
                disabled={checking}
                onClick={onCheckForUpdates}
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
