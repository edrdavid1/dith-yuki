import { useCallback, useEffect, useState } from 'react';
import ColorPicker from '../../components/ColorPicker';
import DropdownMenu from '../../components/common/DropdownMenu';
import Slider from '../../components/common/Slider';
import {
  formatIpcError,
  generateHarmonyPalette,
  logIpcError,
  type GeneratedColorDto,
  type HarmonyRuleName,
} from '../../shared/ipc';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

const RULE_OPTIONS: { value: HarmonyRuleName; label: string }[] = [
  { value: 'Monochromatic', label: 'Monochromatic' },
  { value: 'Analogous', label: 'Analogous' },
  { value: 'Complementary', label: 'Complementary' },
  { value: 'Triadic', label: 'Triadic' },
  { value: 'SplitComplementary', label: 'Split complementary' },
];

export interface HarmonySectionProps {
  onInsert: (hexColors: string[]) => void;
  onError: (message: string | null) => void;
}

/**
 * Harmony preview → Insert replaces Color Lab draft colors (Apply creates Document palette).
 */
export default function HarmonySection({ onInsert, onError }: HarmonySectionProps) {
  const [baseHex, setBaseHex] = useState('CC3344');
  const [rule, setRule] = useState<HarmonyRuleName>('Complementary');
  const [count, setCount] = useState(5);
  /** Half-width spread in degrees for Analogous (converted to radians for IPC). */
  const [spreadDeg, setSpreadDeg] = useState(30);
  const [preview, setPreview] = useState<GeneratedColorDto[]>([]);
  const [busy, setBusy] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerAnchor, setPickerAnchor] = useState<DOMRect | null>(null);

  const refreshPreview = useCallback(async () => {
    setBusy(true);
    try {
      const spreadRad =
        rule === 'Analogous' ? (spreadDeg * Math.PI) / 180 : undefined;
      const colors = await generateHarmonyPalette(baseHex, rule, count, spreadRad);
      setPreview(colors);
      onError(null);
    } catch (err) {
      setPreview([]);
      onError(formatIpcError(err));
      logIpcError('HarmonySection.preview', err);
    } finally {
      setBusy(false);
    }
  }, [baseHex, rule, count, spreadDeg, onError]);

  useEffect(() => {
    const t = window.setTimeout(() => {
      void refreshPreview();
    }, 120);
    return () => window.clearTimeout(t);
  }, [refreshPreview]);

  return (
    <div className={cn('color-lab-column', 'generator-section')}>
      <div className={cn('color-lab-section-title')}>harmonization</div>
      <p className={cn('color-lab-hint')}>
        Insert replaces draft colors. Document palette is created only on Apply.
      </p>

      <div className={cn('generator-swatch-row')}>
        <button
          type="button"
          className={cn('generator-swatch-btn')}
          style={{ backgroundColor: `#${baseHex}` }}
          onClick={(e) => {
            setPickerOpen(true);
            setPickerAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
          }}
          aria-label="Harmony base color"
        />
        <span className={cn('generator-swatch-label')}>base</span>
      </div>

      <DropdownMenu
        value={rule}
        options={RULE_OPTIONS}
        onSelect={(v) => setRule(v as HarmonyRuleName)}
      />

      <Slider
        label="count"
        value={count}
        min={2}
        max={16}
        step={1}
        decimals={0}
        onChange={(v) => setCount(Math.round(v))}
      />

      {rule === 'Analogous' && (
        <Slider
          label="spread (°)"
          value={spreadDeg}
          min={5}
          max={90}
          step={1}
          decimals={0}
          onChange={(v) => setSpreadDeg(Math.round(v))}
        />
      )}

      <div className={cn('generator-preview-strip')} aria-label="Harmony preview">
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

      {pickerOpen && (
        <ColorPicker
          initialColor={baseHex}
          onConfirm={(hex) => setBaseHex(hex)}
          onCancel={() => {
            setPickerOpen(false);
            setPickerAnchor(null);
          }}
          anchorRect={pickerAnchor}
        />
      )}
    </div>
  );
}
