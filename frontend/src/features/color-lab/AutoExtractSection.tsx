import DropdownMenu from '../../components/common/DropdownMenu';
import Slider from '../../components/common/Slider';
import Icon from '../../icons/iconRegistry';
import Tooltip from '../../shared/ui/Tooltip';
import type { ExtractMethod } from './types';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface AutoExtractSectionProps {
  compact?: boolean;
  extractMethod: ExtractMethod;
  extractCount: number;
  chromaWeight: number;
  contrastWeight: number;
  onMethodChange: (method: ExtractMethod) => void;
  onCountChange: (count: number) => void;
  onChromaWeightChange: (value: number) => void;
  onContrastWeightChange: (value: number) => void;
  onExtractRaw: () => void;
  onExtractActual: () => void;
}

export default function AutoExtractSection({
  compact = false,
  extractMethod,
  extractCount,
  chromaWeight,
  contrastWeight,
  onMethodChange,
  onCountChange,
  onChromaWeightChange,
  onContrastWeightChange,
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
        <Slider
          label="chroma"
          value={chromaWeight}
          min={0}
          max={1}
          step={0.05}
          decimals={2}
          onChange={onChromaWeightChange}
        />
        <Slider
          label="contrast"
          value={contrastWeight}
          min={0}
          max={1}
          step={0.05}
          decimals={2}
          onChange={onContrastWeightChange}
        />

        <div className={cn('color-lab-buttons', 'color-lab-buttons-row')}>
          <Tooltip label="Extract from raw frame">
            <button
              type="button"
              onClick={onExtractRaw}
              className={cn('color-lab-button', 'color-lab-button-icon')}
              aria-label="Extract from raw frame"
            >
              <Icon name="row-img" width={16} height={16} />
            </button>
          </Tooltip>
          <Tooltip label="Extract from actual frame">
            <button
              type="button"
              onClick={onExtractActual}
              className={cn('color-lab-button', 'color-lab-button-icon')}
              aria-label="Extract from actual frame"
            >
              <Icon name="image-actual" width={16} height={16} />
            </button>
          </Tooltip>
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
      <Slider
        label="chroma"
        value={chromaWeight}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={onChromaWeightChange}
      />
      <Slider
        label="contrast"
        value={contrastWeight}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={onContrastWeightChange}
      />

      <div className={cn('color-lab-buttons')}>
        <Tooltip label="Extract from raw frame">
          <button
            type="button"
            onClick={onExtractRaw}
            className={cn('color-lab-button')}
            aria-label="Extract from raw frame"
          >
            <Icon name="row-img" width={16} height={16} />
          </button>
        </Tooltip>

        <Tooltip label="Extract from actual frame">
          <button
            type="button"
            onClick={onExtractActual}
            className={cn('color-lab-button')}
            aria-label="Extract from actual frame"
          >
            <Icon name="image-actual" width={16} height={16} />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
