import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export type SvgExportAlgorithm = 'greedy_meshing' | 'contour_tracing';

export interface SvgExportDialogProps {
  isOpen: boolean;
  onExport: (algorithm: SvgExportAlgorithm) => void;
  onClose: () => void;
}

export default function SvgExportDialog({ isOpen, onExport, onClose }: SvgExportDialogProps) {
  const [algorithm, setAlgorithm] = useState<SvgExportAlgorithm>('greedy_meshing');

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  return createPortal(
    <div className={cn('new-project-overlay')} onClick={handleOverlayClick} data-testid="svg-export-overlay">
      <div className={cn('new-project-dialog')} role="dialog" aria-modal="true" aria-label="SVG export">
        <DialogTitlebar title="Export SVG" onClose={onClose} />
        <form
          className={cn('new-project-body')}
          onSubmit={(e) => {
            e.preventDefault();
            onExport(algorithm);
          }}
        >
          <fieldset className={cn('new-project-field')}>
            <legend>Mode</legend>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="svg-algorithm"
                checked={algorithm === 'greedy_meshing'}
                onChange={() => setAlgorithm('greedy_meshing')}
              />
              Pixel Grid
            </label>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="svg-algorithm"
                checked={algorithm === 'contour_tracing'}
                onChange={() => setAlgorithm('contour_tracing')}
              />
              Contour
            </label>
          </fieldset>
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className={cn('new-project-btn', 'new-project-btn-primary')}>
              Export
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}
