import { useEffect, useMemo, useState } from 'react';
import DropdownMenu from '../../components/common/DropdownMenu';
import {
  listBuiltinPalettes,
  logIpcError,
  type BuiltinPaletteDto,
} from '../../shared/ipc';
import { toHex } from '../../types/effects';
import styles from './ColorLabWindow.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

export interface BuiltinPresetsSectionProps {
  onImportBuiltin: (id: string) => void;
  disabled?: boolean;
}

function PresetSwatches({
  colors,
  presetId,
}: {
  colors: BuiltinPaletteDto['colors'];
  presetId: string;
}) {
  return (
    <span className={cn('builtin-preset-swatches')} aria-hidden>
      {colors.map(([r, g, b], i) => (
        <span
          key={`${presetId}-${i}`}
          className={cn('builtin-preset-swatch')}
          style={{ backgroundColor: toHex(r, g, b) }}
        />
      ))}
    </span>
  );
}

/**
 * Built-in retro palettes — colors come only from `list_builtin_palettes`.
 */
export default function BuiltinPresetsSection({
  onImportBuiltin,
  disabled = false,
}: BuiltinPresetsSectionProps) {
  const [presets, setPresets] = useState<BuiltinPaletteDto[]>([]);
  const [selectedId, setSelectedId] = useState('');

  useEffect(() => {
    let cancelled = false;
    listBuiltinPalettes()
      .then((list) => {
        if (cancelled) return;
        setPresets(list);
        setSelectedId((prev) => prev || list[0]?.id || '');
      })
      .catch((err) => {
        if (!cancelled) logIpcError('BuiltinPresetsSection.list', err);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selected = useMemo(
    () => presets.find((p) => p.id === selectedId) ?? presets[0],
    [presets, selectedId],
  );

  const options = useMemo(
    () => presets.map((p) => ({ value: p.id, label: p.name })),
    [presets],
  );

  if (presets.length === 0 || !selected) return null;

  return (
    <div className={cn('color-lab-column', 'builtin-presets')}>
      <div className={cn('color-lab-section-title')}>built-in retro palettes</div>
      <DropdownMenu
        value={selected.id}
        options={options}
        disabled={disabled}
        onSelect={(id) => {
          setSelectedId(id);
          onImportBuiltin(id);
        }}
        selectedContent={
          <span className={cn('builtin-preset-field')}>
            <PresetSwatches colors={selected.colors} presetId={selected.id} />
            <span className={cn('builtin-preset-name')}>{selected.name}</span>
          </span>
        }
        renderOption={(option) => {
          const preset = presets.find((p) => p.id === option.value);
          if (!preset) return option.label;
          return (
            <span className={cn('builtin-preset-field')}>
              <PresetSwatches colors={preset.colors} presetId={preset.id} />
              <span className={cn('builtin-preset-name')}>{preset.name}</span>
            </span>
          );
        }}
      />
    </div>
  );
}
