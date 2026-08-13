import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import titlebarStyles from '../shared/ui/WindowTitlebar.module.css';
import { bind } from '../shared/ui/cn';

const cn = bind({ ...styles, ...titlebarStyles });

export interface UnsavedGuardDialogProps {
  isOpen: boolean;
  basename: string;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

export default function UnsavedGuardDialog({
  isOpen,
  basename,
  onSave,
  onDiscard,
  onCancel,
}: UnsavedGuardDialogProps) {
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onCancel();
    },
    [onCancel]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
      }
    },
    [onCancel]
  );

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="unsaved-guard-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="unsaved-guard-title"
        onKeyDown={handleKeyDown}
      >
        <div className={cn('window-titlebar')}>
          <button
            className={cn('window-titlebar-btn')}
            onClick={onCancel}
            aria-label="Close"
            type="button"
          />
          <span className={cn('window-title')} id="unsaved-guard-title">
            Save changes?
          </span>
        </div>
        <div className={cn('new-project-body')}>
          <p className={cn('new-project-field')}>
            Save changes to {basename} before closing?
          </p>
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onCancel}>
              Cancel
            </button>
            <button type="button" className={cn('new-project-btn')} onClick={onDiscard}>
              Don’t Save
            </button>
            <button
              type="button"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              onClick={onSave}
            >
              Save
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
