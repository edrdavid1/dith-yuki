import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../document/NewProjectDialog.module.css';
import { bind } from '../../shared/ui/cn';
import { DialogTitlebar } from '../../shared/ui/WindowTitlebar';
import PreferencesPanel from './PreferencesPanel';

const cn = bind(styles);

export interface PreferencesDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

export default function PreferencesDialog({ isOpen, onClose }: PreferencesDialogProps) {
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
      data-testid="preferences-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="preferences-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Preferences" titleId="preferences-title" onClose={onClose} />
        <div className={cn('new-project-body')}>
          <PreferencesPanel />
        </div>
      </div>
    </div>,
    document.body
  );
}
