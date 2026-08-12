import SimpleBar from 'simplebar-react';
import type { ColorEntry } from './types';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface PaletteManualEditorProps {
  colors: ColorEntry[];
  canAddColor: boolean;
  compact?: boolean;
  showSectionTitle?: boolean;
  onChange: (index: number, hex: string) => void;
  onDelete: (index: number) => void;
  onAdd: () => void;
  onOpenPicker: (index: number, e: React.MouseEvent) => void;
}

export default function PaletteManualEditor({
  colors,
  canAddColor,
  compact = false,
  showSectionTitle = true,
  onChange,
  onDelete,
  onAdd,
  onOpenPicker,
}: PaletteManualEditorProps) {
  const list = (
    <div className={cn('color-lab-colors-grid', compact && 'color-lab-colors-grid-compact')}>
      {colors.map((entry, idx) => (
        <div key={idx} className={cn('color-lab-color-row', compact && 'color-lab-color-row-compact')}>
          <span className={cn('color-lab-color-number')}>{idx + 1}</span>

          <div
            className={cn(
              'color-lab-color-preview',
              compact && 'color-lab-color-preview-compact',
              !entry.valid && 'invalid'
            )}
            onClick={(e) => onOpenPicker(idx, e)}
            style={{ cursor: 'pointer', background: entry.valid ? entry.hex : undefined }}
          />

          <input
            type="text"
            value={entry.hex}
            onChange={(e) => onChange(idx, e.target.value)}
            className={cn('color-lab-color-input', !entry.valid && 'invalid')}
            maxLength={7}
          />

          <button
            type="button"
            onClick={() => onDelete(idx)}
            className={cn('color-lab-button', 'color-lab-delete-btn')}
            title="Remove color"
          >
            <img src="/icons/delete-con.svg" style={{ width: '14px', height: '14px' }} alt="" />
          </button>
        </div>
      ))}

      <button
        type="button"
        onClick={onAdd}
        disabled={!canAddColor}
        className={cn(compact ? 'color-lab-add-link' : 'color-lab-button', !compact && 'color-lab-add-btn')}
      >
        add color +
      </button>
    </div>
  );

  return (
    <>
      <div className={cn('color-lab-manual-section')}>
        {showSectionTitle && <div className={cn('color-lab-section-title')}>manual edit</div>}

        {compact ? (
          list
        ) : (
          <SimpleBar className={cn('color-lab-colors-container')} style={{ maxHeight: '200px' }}>
            {list}
          </SimpleBar>
        )}
      </div>

      <div className={cn('color-lab-preview-bar')}>
        {colors
          .filter((c) => c.valid)
          .map((c, idx) => (
            <div
              key={idx}
              className={cn('color-lab-preview-color')}
              style={{ backgroundColor: c.hex }}
              title={c.hex}
            />
          ))}
      </div>
    </>
  );
}
