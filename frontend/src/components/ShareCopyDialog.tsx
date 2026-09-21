import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import type { ShareProjectCopyOptions } from '../shared/ipc/project';

const cn = bind(styles);

export interface ShareCopyDialogProps {
  isOpen: boolean;
  onExport: (opts: ShareProjectCopyOptions) => void;
  onCancel: () => void;
}

/** SPEC §11 defaults: strip on, originals off, author off, compact off. */
const DEFAULTS: Required<ShareProjectCopyOptions> = {
  stripMetadata: true,
  includeOriginalImages: false,
  includeAuthor: false,
  compact: false,
};

export default function ShareCopyDialog({ isOpen, onExport, onCancel }: ShareCopyDialogProps) {
  const [stripMetadata, setStripMetadata] = useState(DEFAULTS.stripMetadata);
  const [includeOriginalImages, setIncludeOriginalImages] = useState(
    DEFAULTS.includeOriginalImages
  );
  const [includeAuthor, setIncludeAuthor] = useState(DEFAULTS.includeAuthor);
  const [compact, setCompact] = useState(DEFAULTS.compact);

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

  const handleExport = useCallback(() => {
    onExport({
      stripMetadata,
      includeOriginalImages,
      includeAuthor,
      compact,
    });
  }, [compact, includeAuthor, includeOriginalImages, onExport, stripMetadata]);

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="share-copy-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="share-copy-title"
        onKeyDown={handleKeyDown}
        data-testid="share-copy-dialog"
      >
        <DialogTitlebar title="Share Copy" titleId="share-copy-title" onClose={onCancel} />
        <div className={cn('new-project-body')}>
          <p className={cn('new-project-field')}>
            Export a privacy-scrubbed project for sharing. This does not change your open file.
          </p>
          <label className={cn('new-project-field')} style={{ display: 'flex', gap: 8 }}>
            <input
              type="checkbox"
              checked={stripMetadata}
              onChange={(e) => setStripMetadata(e.target.checked)}
              data-testid="share-copy-strip-metadata"
            />
            Strip metadata from embedded images
          </label>
          <label className={cn('new-project-field')} style={{ display: 'flex', gap: 8 }}>
            <input
              type="checkbox"
              checked={includeOriginalImages}
              onChange={(e) => setIncludeOriginalImages(e.target.checked)}
              data-testid="share-copy-include-originals"
            />
            Include original images (not stored today)
          </label>
          <label className={cn('new-project-field')} style={{ display: 'flex', gap: 8 }}>
            <input
              type="checkbox"
              checked={includeAuthor}
              onChange={(e) => setIncludeAuthor(e.target.checked)}
              data-testid="share-copy-include-author"
            />
            Include author
          </label>
          <label className={cn('new-project-field')} style={{ display: 'flex', gap: 8 }}>
            <input
              type="checkbox"
              checked={compact}
              onChange={(e) => setCompact(e.target.checked)}
              data-testid="share-copy-compact"
            />
            Compact JSON
          </label>
        </div>
        <div className={cn('new-project-footer')}>
          <button type="button" className={cn('new-project-btn')} onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className={cn('new-project-btn', 'new-project-btn-primary')}
            onClick={handleExport}
            data-testid="share-copy-export"
          >
            Export…
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}

/** Exported for tests — SPEC §11 default matrix. */
export function shareCopyDefaultOptions(): Required<ShareProjectCopyOptions> {
  return { ...DEFAULTS };
}
