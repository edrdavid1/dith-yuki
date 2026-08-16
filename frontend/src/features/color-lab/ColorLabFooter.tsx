import Icon from '../../icons/iconRegistry';
import Tooltip from '../../shared/ui/Tooltip';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface ColorLabFooterProps {
  cancelLabel: 'Reset' | 'Cancel';
  onSort: () => void;
  onCancel: () => void;
  onApply: () => void;
}

export default function ColorLabFooter({
  cancelLabel,
  onSort,
  onCancel,
  onApply,
}: ColorLabFooterProps) {
  return (
    <div className={cn("color-lab-utility-section")}>
      <div className={cn("color-lab-utility-grid")}>
        <Tooltip label="Sort by brightness">
          <button
            type="button"
            onClick={onSort}
            className={cn("color-lab-button", "color-lab-button-icon")}
            aria-label="Sort by brightness"
          >
            <Icon name="sort" width={16} height={16} />
          </button>
        </Tooltip>
        <Tooltip label="Auto interpolate">
          <button
            type="button"
            disabled
            className={cn("color-lab-button", "color-lab-button-icon")}
            aria-label="Auto interpolate"
          >
            <Icon name="auto-interpolate" width={16} height={16} />
          </button>
        </Tooltip>
      </div>

      <div className={cn("color-lab-footer")}>
        <button
          type="button"
          onClick={onCancel}
          className={cn("color-lab-button", "color-lab-cancel-btn")}
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          onClick={onApply}
          className={cn("color-lab-button", "color-lab-apply-btn")}
        >
          Apply
        </button>
      </div>
    </div>
  );
}
