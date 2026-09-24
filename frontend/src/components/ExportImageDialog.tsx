import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export type ImageExportFormat = 'PNG' | 'JPEG' | 'WEBP' | 'BMP' | 'TIFF' | 'SVG';
export type SvgExportAlgorithm = 'greedy_meshing' | 'contour_tracing';

export interface ImageExportOptions {
  format: ImageExportFormat;
  quality?: number;
  svg_algorithm?: SvgExportAlgorithm;
}

export interface ExportImageDialogProps {
  isOpen: boolean;
  onExport: (options: ImageExportOptions) => void;
  onClose: () => void;
}

const FORMATS: { value: ImageExportFormat; label: string }[] = [
  { value: 'PNG', label: 'PNG (.png) — lossless, transparency' },
  { value: 'JPEG', label: 'JPEG (.jpg) — lossy, no transparency' },
  { value: 'WEBP', label: 'WebP (.webp) — lossless, transparency' },
  { value: 'BMP', label: 'BMP (.bmp)' },
  { value: 'TIFF', label: 'TIFF (.tif)' },
  { value: 'SVG', label: 'SVG (.svg) — vectorized' },
];

export function extensionForFormat(format: ImageExportFormat): string {
  switch (format) {
    case 'JPEG':
      return 'jpg';
    case 'TIFF':
      return 'tif';
    case 'WEBP':
      return 'webp';
    case 'BMP':
      return 'bmp';
    case 'SVG':
      return 'svg';
    case 'PNG':
    default:
      return 'png';
  }
}

export default function ExportImageDialog({ isOpen, onExport, onClose }: ExportImageDialogProps) {
  const [format, setFormat] = useState<ImageExportFormat>('PNG');
  const [quality, setQuality] = useState(90);
  const [svgAlgorithm, setSvgAlgorithm] = useState<SvgExportAlgorithm>('greedy_meshing');

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
      data-testid="image-export-overlay"
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-label="Export Image"
      >
        <DialogTitlebar title="Export Image" onClose={onClose} />
        <form
          className={cn('new-project-body')}
          onSubmit={(e) => {
            e.preventDefault();
            onExport({
              format,
              quality: format === 'JPEG' ? quality : undefined,
              svg_algorithm: format === 'SVG' ? svgAlgorithm : undefined,
            });
          }}
        >
          <fieldset className={cn('new-project-field')}>
            <legend>Format</legend>
            {FORMATS.map((f) => (
              <label key={f.value} className={cn('new-project-radio')}>
                <input
                  type="radio"
                  name="image-format"
                  checked={format === f.value}
                  onChange={() => setFormat(f.value)}
                />
                {f.label}
              </label>
            ))}
          </fieldset>

          {format === 'JPEG' && (
            <label className={cn('new-project-field')}>
              Quality ({quality})
              <input
                type="range"
                min={1}
                max={100}
                value={quality}
                onChange={(e) => setQuality(Number(e.target.value))}
                className={cn('new-project-range')}
              />
            </label>
          )}

          {format === 'SVG' && (
            <fieldset className={cn('new-project-field')}>
              <legend>SVG mode</legend>
              <label className={cn('new-project-radio')}>
                <input
                  type="radio"
                  name="svg-algorithm"
                  checked={svgAlgorithm === 'greedy_meshing'}
                  onChange={() => setSvgAlgorithm('greedy_meshing')}
                />
                Pixel Grid
              </label>
              <label className={cn('new-project-radio')}>
                <input
                  type="radio"
                  name="svg-algorithm"
                  checked={svgAlgorithm === 'contour_tracing'}
                  onChange={() => setSvgAlgorithm('contour_tracing')}
                />
                Contour
              </label>
            </fieldset>
          )}

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
