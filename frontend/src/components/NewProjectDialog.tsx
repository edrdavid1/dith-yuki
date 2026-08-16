import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  DEFAULT_DOCUMENT_HEIGHT,
  DEFAULT_DOCUMENT_WIDTH,
  MAX_DOCUMENT_DIMENSION,
} from '../shared/documentLimits';
import type { BlankBackground } from '../shared/ipc/document';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export interface NewProjectDialogProps {
  isOpen: boolean;
  onCreate: (args: {
    width: number;
    height: number;
    background: BlankBackground;
  }) => void | Promise<void>;
  onClose: () => void;
}

function parseDimension(raw: string): number | null {
  const trimmed = raw.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  if (!Number.isInteger(n) || n < 1 || n > MAX_DOCUMENT_DIMENSION) return null;
  return n;
}

function NewProjectDialog({ isOpen, onCreate, onClose }: NewProjectDialogProps) {
  const [width, setWidth] = useState(String(DEFAULT_DOCUMENT_WIDTH));
  const [height, setHeight] = useState(String(DEFAULT_DOCUMENT_HEIGHT));
  const [background, setBackground] = useState<BlankBackground>('transparent');
  const widthRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!isOpen) return;
    setWidth(String(DEFAULT_DOCUMENT_WIDTH));
    setHeight(String(DEFAULT_DOCUMENT_HEIGHT));
    setBackground('transparent');
    requestAnimationFrame(() => widthRef.current?.focus());
  }, [isOpen]);

  const parsedWidth = parseDimension(width);
  const parsedHeight = parseDimension(height);
  const valid = parsedWidth !== null && parsedHeight !== null;
  const error = valid
    ? null
    : `Width and height must be integers from 1 to ${MAX_DOCUMENT_DIMENSION}`;

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      if (parsedWidth === null || parsedHeight === null) return;
      void onCreate({ width: parsedWidth, height: parsedHeight, background });
    },
    [background, onCreate, parsedHeight, parsedWidth]
  );

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
      data-testid="new-project-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-label="New Project"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="New Project" onClose={onClose} />

        <form className={cn('new-project-body')} onSubmit={handleSubmit}>
          <label className={cn('new-project-field')}>
            <span>Width (px)</span>
            <input
              ref={widthRef}
              className={cn('new-project-input')}
              type="text"
              inputMode="numeric"
              value={width}
              onChange={(e) => setWidth(e.target.value)}
              aria-invalid={parsedWidth === null}
              aria-label="Width"
            />
          </label>
          <label className={cn('new-project-field')}>
            <span>Height (px)</span>
            <input
              className={cn('new-project-input')}
              type="text"
              inputMode="numeric"
              value={height}
              onChange={(e) => setHeight(e.target.value)}
              aria-invalid={parsedHeight === null}
              aria-label="Height"
            />
          </label>
          <fieldset className={cn('new-project-field')}>
            <legend>Background</legend>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="background"
                value="transparent"
                checked={background === 'transparent'}
                onChange={() => setBackground('transparent')}
              />
              Transparent
            </label>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="background"
                value="white"
                checked={background === 'white'}
                onChange={() => setBackground('white')}
              />
              White
            </label>
          </fieldset>
          {error && (
            <p className={cn('new-project-error')} role="alert">
              {error}
            </p>
          )}
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              disabled={!valid}
            >
              Create
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}

export default NewProjectDialog;
