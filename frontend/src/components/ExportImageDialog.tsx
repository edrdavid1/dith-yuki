import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';

const cn = bind(styles);

export type ImageExportFormat = 'PNG' | 'PNG8' | 'JPEG' | 'WEBP' | 'BMP' | 'TIFF' | 'SVG';
export type SvgExportAlgorithm = 'greedy_meshing' | 'contour_tracing';

export interface ImageExportOptions {
  format: ImageExportFormat;
  quality?: number;
  svg_algorithm?: SvgExportAlgorithm;
  /** When both set, export processes each sprite-sheet cell independently (ordered only). */
  sprite_cell_w?: number;
  sprite_cell_h?: number;
}

export interface ExportImageDialogProps {
  isOpen: boolean;
  onExport: (options: ImageExportOptions) => void;
  onClose: () => void;
}

const FORMATS: { value: ImageExportFormat; label: string }[] = [
  { value: 'PNG', label: 'PNG (.png) — lossless, transparency' },
  {
    value: 'PNG8',
    label: 'PNG8 indexed (.png) — palette in file (Strict / Mixed / Simple)',
  },
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
    case 'PNG8':
    default:
      return 'png';
  }
}

export default function ExportImageDialog({ isOpen, onExport, onClose }: ExportImageDialogProps) {
  const [format, setFormat] = useState<ImageExportFormat>('PNG');
  const [quality, setQuality] = useState(90);
  const [svgAlgorithm, setSvgAlgorithm] = useState<SvgExportAlgorithm>('greedy_meshing');
  const [spriteCells, setSpriteCells] = useState(false);
  const [cellW, setCellW] = useState(32);
  const [cellH, setCellH] = useState(32);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  const spriteOk = !spriteCells || format === 'PNG' || format === 'PNG8';

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
            if (!spriteOk) return;
            onExport({
              format,
              quality: format === 'JPEG' ? quality : undefined,
              svg_algorithm: format === 'SVG' ? svgAlgorithm : undefined,
              ...(spriteCells
                ? { sprite_cell_w: Math.max(1, Math.round(cellW)), sprite_cell_h: Math.max(1, Math.round(cellH)) }
                : {}),
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

          <label className={cn('new-project-radio')}>
            <input
              type="checkbox"
              checked={spriteCells}
              onChange={(e) => setSpriteCells(e.target.checked)}
            />
            Sprite-sheet cells (ordered dither, same phase per cell)
          </label>
          {spriteCells && (
            <>
              <div className={cn('new-project-field')}>
                <label htmlFor="sprite-cell-w">Cell width (px)</label>
                <input
                  id="sprite-cell-w"
                  className={cn('new-project-input')}
                  type="number"
                  min={1}
                  value={cellW}
                  onChange={(e) => setCellW(Number(e.target.value))}
                />
              </div>
              <div className={cn('new-project-field')}>
                <label htmlFor="sprite-cell-h">Cell height (px)</label>
                <input
                  id="sprite-cell-h"
                  className={cn('new-project-input')}
                  type="number"
                  min={1}
                  value={cellH}
                  onChange={(e) => setCellH(Number(e.target.value))}
                />
              </div>
              {!spriteOk && (
                <p style={{ margin: 0, fontSize: 11, opacity: 0.8 }}>
                  Cell export supports PNG and PNG8 only.
                </p>
              )}
            </>
          )}

          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              disabled={!spriteOk}
            >
              Export…
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}
