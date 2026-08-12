import { useCallback, useEffect, useState } from 'react';
import ColorPicker from '../../components/ColorPicker';
import Slider from '../../components/common/Slider';
import {
  formatIpcError,
  generateRampPalette,
  logIpcError,
  type GeneratedColorDto,
} from '../../shared/ipc';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface RampGeneratorSectionProps {
  onInsert: (hexColors: string[]) => void;
  onError: (message: string | null) => void;
}

type PickerTarget = 'from' | 'to' | null;

/**
 * Oklab ramp preview → Insert replaces Color Lab draft colors (Apply creates Document palette).
 */
export default function RampGeneratorSection({ onInsert, onError }: RampGeneratorSectionProps) {
  const [fromHex, setFromHex] = useState('000000');
  const [toHex, setToHex] = useState('FFFFFF');
  const [steps, setSteps] = useState(8);
  const [preview, setPreview] = useState<GeneratedColorDto[]>([]);
  const [busy, setBusy] = useState(false);
  const [picker, setPicker] = useState<PickerTarget>(null);
  const [pickerAnchor, setPickerAnchor] = useState<DOMRect | null>(null);

  const refreshPreview = useCallback(async () => {
    setBusy(true);
    try {
      const colors = await generateRampPalette(fromHex, toHex, steps);
      setPreview(colors);
      onError(null);
    } catch (err) {
      setPreview([]);
      onError(formatIpcError(err));
      logIpcError('RampGeneratorSection.preview', err);
    } finally {
      setBusy(false);
    }
  }, [fromHex, toHex, steps, onError]);

  useEffect(() => {
    const t = window.setTimeout(() => {
      void refreshPreview();
    }, 120);
    return () => window.clearTimeout(t);
  }, [refreshPreview]);

  const openPicker = (target: 'from' | 'to', e: React.MouseEvent) => {
    setPicker(target);
    setPickerAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
  };

  return (
    <div className={cn('color-lab-column', 'generator-section')}>
      <div className={cn('color-lab-section-title')}>ramps generator</div>
      <p className={cn('color-lab-hint')}>
        Insert replaces draft colors. Document palette is created only on Apply.
      </p>

      <div className={cn('generator-swatch-row')}>
        <button
          type="button"
          className={cn('generator-swatch-btn')}
          style={{ backgroundColor: `#${fromHex}` }}
          onClick={(e) => openPicker('from', e)}
          aria-label="Ramp from color"
        />
        <span className={cn('generator-swatch-label')}>→</span>
        <button
          type="button"
          className={cn('generator-swatch-btn')}
          style={{ backgroundColor: `#${toHex}` }}
          onClick={(e) => openPicker('to', e)}
          aria-label="Ramp to color"
        />
      </div>

      <Slider
        label="steps"
        value={steps}
        min={2}
        max={32}
        step={1}
        decimals={0}
        onChange={(v) => setSteps(Math.round(v))}
      />

      <div className={cn('generator-preview-strip')} aria-label="Ramp preview">
        {preview.map((c, i) => (
          <span
            key={`${c.hex}-${i}`}
            className={cn('generator-preview-cell')}
            style={{ backgroundColor: c.hex }}
            title={c.hex}
          />
        ))}
      </div>

      <button
        type="button"
        className={cn('color-lab-button')}
        disabled={busy || preview.length === 0}
        onClick={() => onInsert(preview.map((c) => c.hex))}
      >
        Insert into draft
      </button>

      {picker && (
        <ColorPicker
          initialColor={picker === 'from' ? fromHex : toHex}
          onConfirm={(hex) => {
            if (picker === 'from') setFromHex(hex);
            else setToHex(hex);
          }}
          onCancel={() => {
            setPicker(null);
            setPickerAnchor(null);
          }}
          anchorRect={pickerAnchor}
        />
      )}
    </div>
  );
}
