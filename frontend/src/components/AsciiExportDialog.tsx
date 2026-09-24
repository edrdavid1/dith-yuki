import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import type { AsciiExportFormat } from '../shared/ipc/ascii';

const cn = bind(styles);

export interface AsciiExportDialogProps {
  isOpen: boolean;
  onExport: (format: AsciiExportFormat) => void;
  onClose: () => void;
}

const FORMATS: { value: AsciiExportFormat; label: string }[] = [
  { value: 'txt', label: 'Plain text (.txt)' },
  { value: 'ansi', label: 'ANSI (.ans)' },
  { value: 'html', label: 'HTML (.html)' },
  { value: 'svg', label: 'SVG (.svg)' },
  { value: 'png', label: 'PNG (.png)' },
  { value: 'json', label: 'JSON (.json)' },
];

export default function AsciiExportDialog({ isOpen, onExport, onClose }: AsciiExportDialogProps) {
  const [format, setFormat] = useState<AsciiExportFormat>('txt');

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="ascii-export-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-label="Export ASCII"
      >
        <DialogTitlebar title="Export ASCII" onClose={onClose} />
        <form
          className={cn('new-project-body')}
          onSubmit={(e) => {
            e.preventDefault();
            onExport(format);
          }}
        >
          <fieldset className={cn('new-project-field')}>
            <legend>Format</legend>
            {FORMATS.map((f) => (
              <label key={f.value} className={cn('new-project-radio')}>
                <input
                  type="radio"
                  name="ascii-format"
                  checked={format === f.value}
                  onChange={() => setFormat(f.value)}
                />
                {f.label}
              </label>
            ))}
          </fieldset>
          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className={cn('new-project-btn', 'new-project-btn-primary')}>
              Export…
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}
