import { useMemo } from 'react';
import DropdownMenu, { type DropdownOption } from '../../components/common/DropdownMenu';
import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';
import { toHex } from '../../types/effects';
import styles from './ColorLabWindow.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

export const PALETTE_NEW_VALUE = 'new';
export const builtinKey = (id: string) => `builtin:${id}`;
export const savedKey = (id: number) => `saved:${id}`;

export interface PaletteManagerSectionProps {
  builtins: BuiltinPaletteDto[];
  saved: PaletteDto[];
  selectedPaletteId: number | null;
  onSelectNew: () => void;
  onSelectSaved: (id: number) => void;
  onSelectBuiltin: (id: string) => void;
  previewColors?: [number, number, number][];
}

function Swatches({
  colors,
  swatchId,
}: {
  colors: [number, number, number][];
  swatchId: string;
}) {
  if (colors.length === 0) {
    return <span className={cn('builtin-preset-swatches')} aria-hidden />;
  }
  return (
    <span className={cn('builtin-preset-swatches')} aria-hidden>
      {colors.map(([r, g, b], i) => (
        <span
          key={`${swatchId}-${i}`}
          className={cn('builtin-preset-swatch')}
          style={{ backgroundColor: toHex(r, g, b) }}
        />
      ))}
    </span>
  );
}

function Row({
  colors,
  name,
  id,
}: {
  colors: [number, number, number][];
  name: string;
  id: string;
}) {
  return (
    <span className={cn('builtin-preset-field')}>
      <Swatches colors={colors} swatchId={id} />
      <span className={cn('builtin-preset-name')}>{name}</span>
    </span>
  );
}

/**
 * Single palette picker: New + built-in templates + document (saved) palettes.
 */
export default function PaletteManagerSection({
  builtins,
  saved,
  selectedPaletteId,
  onSelectNew,
  onSelectSaved,
  onSelectBuiltin,
  previewColors,
}: PaletteManagerSectionProps) {
  const value =
    selectedPaletteId !== null ? savedKey(selectedPaletteId) : PALETTE_NEW_VALUE;

  const selectedSaved = saved.find((p) => p.id === selectedPaletteId) ?? null;

  const options = useMemo<DropdownOption[]>(() => {
    const list: DropdownOption[] = [
      { value: PALETTE_NEW_VALUE, label: 'New palette', group: 'Draft' },
    ];
    for (const p of builtins) {
      list.push({ value: builtinKey(p.id), label: p.name, group: 'Built-in' });
    }
    for (const p of saved) {
      list.push({ value: savedKey(p.id), label: p.name, group: 'Saved' });
    }
    return list;
  }, [builtins, saved]);

  const fieldColors =
    previewColors && previewColors.length > 0
      ? previewColors
      : selectedSaved?.colors ?? [];

  return (
    <div className={cn('color-lab-column', 'builtin-presets')}>
      <div className={cn('color-lab-section-title')}>palettes</div>
      <DropdownMenu
        value={value}
        options={options}
        onSelect={(v) => {
          if (v === PALETTE_NEW_VALUE) {
            onSelectNew();
            return;
          }
          if (v.startsWith('builtin:')) {
            onSelectBuiltin(v.slice('builtin:'.length));
            return;
          }
          if (v.startsWith('saved:')) {
            const id = Number(v.slice('saved:'.length));
            if (Number.isFinite(id)) onSelectSaved(id);
          }
        }}
        selectedContent={
          selectedSaved ? (
            <Row colors={fieldColors} name={selectedSaved.name} id={`saved-${selectedSaved.id}`} />
          ) : (
            <Row colors={fieldColors} name="New palette" id="new" />
          )
        }
        renderOption={(option) => {
          if (option.value === PALETTE_NEW_VALUE) {
            return option.label;
          }
          if (option.value.startsWith('builtin:')) {
            const preset = builtins.find((p) => builtinKey(p.id) === option.value);
            if (!preset) return option.label;
            return <Row colors={preset.colors} name={preset.name} id={preset.id} />;
          }
          const palette = saved.find((p) => savedKey(p.id) === option.value);
          if (!palette) return option.label;
          return <Row colors={palette.colors} name={palette.name} id={`saved-${palette.id}`} />;
        }}
      />
    </div>
  );
}
