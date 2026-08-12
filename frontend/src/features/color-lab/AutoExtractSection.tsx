import DropdownMenu from '../../components/common/DropdownMenu';
import Slider from '../../components/common/Slider';
import Icon from '../../icons/iconRegistry';
import type { ExtractMethod } from './types';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface AutoExtractSectionProps {
  compact?: boolean;
  extractMethod: ExtractMethod;
  extractCount: number;
  onMethodChange: (method: ExtractMethod) => void;
  onCountChange: (count: number) => void;
  onExtractRaw: () => void;
  onExtractActual: () => void;
}

export default function AutoExtractSection({
  compact = false,
  extractMethod,
  extractCount,
  onMethodChange,
  onCountChange,
  onExtractRaw,
  onExtractActual,
}: AutoExtractSectionProps) {
  if (compact) {
    return (
      <div className={cn('color-lab-column')}>
        <Slider
          label="color count"
          value={extractCount}
          min={2}
          max={64}
          step={1}
          decimals={0}
          onChange={(v) => onCountChange(Math.round(v))}
        />

        <div className={cn('color-lab-buttons', 'color-lab-buttons-row')}>
          <button type="button" onClick={onExtractRaw} className={cn('color-lab-button')}>
            Extract from row
          </button>
          <button type="button" onClick={onExtractActual} className={cn('color-lab-button')}>
            Extract from actual
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={cn('color-lab-column')}>
      <div className={cn('color-lab-section-title')}>auto extract</div>

      <DropdownMenu
        value={extractMethod}
        options={[
          { value: 'MedianCut', label: 'Median Cut' },
          { value: 'KMeans', label: 'K-Means' },
        ]}
        onSelect={(v) => onMethodChange(v as ExtractMethod)}
      />

      <Slider
        label="color count"
        value={extractCount}
        min={2}
        max={64}
        step={1}
        decimals={0}
        onChange={(v) => onCountChange(Math.round(v))}
      />

      <div className={cn('color-lab-buttons')}>
        <button type="button" onClick={onExtractRaw} className={cn('color-lab-button')}>
          <Icon name="row-img" width={16} height={16} />
          Extract from raw frame
        </button>

        <button type="button" onClick={onExtractActual} className={cn('color-lab-button')}>
          <Icon name="row-img" width={16} height={16} />
          Extract from actual frame
        </button>
      </div>
    </div>
  );
}
