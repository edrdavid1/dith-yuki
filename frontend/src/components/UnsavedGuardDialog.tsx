import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import {
  displayNameForUnsaved,
  type UnsavedDocumentRef,
} from '../shared/unsavedGuard';

const cn = bind(styles);

export interface UnsavedGuardDialogProps {
  isOpen: boolean;
  /** Single-doc mode: basename shown in the body copy. */
  basename?: string;
  /** Multi-doc mode: checklist of dirty documents (all selected by default upstream). */
  documents?: UnsavedDocumentRef[];
  selectedIds?: number[];
  onToggleSelected?: (id: number) => void;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

export default function UnsavedGuardDialog({
  isOpen,
  basename = 'Untitled',
  documents,
  selectedIds,
  onToggleSelected,
  onSave,
  onDiscard,
  onCancel,
}: UnsavedGuardDialogProps) {
  const multi = (documents?.length ?? 0) > 1;
  const selected = selectedIds ?? [];

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

  const saveDisabled = multi && selected.length === 0;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="unsaved-guard-overlay"
    >
      <div
        className={cn('new-project-dialog', multi && 'new-project-dialog-wide')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="unsaved-guard-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Save changes?" titleId="unsaved-guard-title" onClose={onCancel} />
        <div className={cn('new-project-body')} style={multi ? { padding: '14px 12px 12px' } : undefined}>
          {multi ? (
            <>
              <p className={cn('new-project-field')}>
                The following documents have unsaved changes:
              </p>
              <ul
                className={cn('new-project-field')}
                style={{ listStyle: 'none', margin: 0, padding: 0, gap: 6 }}
                data-testid="unsaved-guard-doc-list"
              >
                {documents!.map((doc) => {
                  const checked = selected.includes(doc.id);
                  const name = displayNameForUnsaved(doc);
                  return (
                    <li key={doc.id}>
                      <label className={cn('new-project-radio')}>
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={() => onToggleSelected?.(doc.id)}
                          aria-label={name}
                        />
                        {name}
                      </label>
                    </li>
                  );
                })}
              </ul>
            </>
          ) : (
            <p className={cn('new-project-field')}>
              Save changes to {basename} before closing?
            </p>
          )}
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onCancel}>
              Cancel
            </button>
            <button type="button" className={cn('new-project-btn')} onClick={onDiscard}>
              {multi ? 'Discard All' : 'Don’t Save'}
            </button>
            <button
              type="button"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              onClick={onSave}
              disabled={saveDisabled}
            >
              {multi ? 'Save Selected & Quit' : 'Save'}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
