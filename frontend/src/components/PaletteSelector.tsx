import { useState, useEffect } from 'react';
import { listPalettes } from '../ipc/commands';
import type { PaletteDto } from '../ipc/commands';
import DropdownMenu from './common/DropdownMenu';
import { useAppSelector } from '../app/hooks';

export interface PaletteSelectorProps {
  /** Currently selected palette ID, or null for "None" */
  selectedPaletteId: number | null;
  /** Whether to show a "None" option (DitherV2 = true, PaletteQuantize = false) */
  allowNone: boolean;
  /** Callback when the user selects a palette (or null for "None") */
  onChange: (paletteId: number | null) => void;
  /** Optional label override (defaults to "Palette") */
  label?: string;
  /** When this value changes, the palette list is re-fetched */
  refreshKey?: number;
}

/**
 * Dropdown for document palettes. Empty selection with allowNone=false
 * displays palettesSlice.lastCreatedId when set.
 */
function PaletteSelector({
  selectedPaletteId,
  allowNone,
  onChange,
  label = 'Palette',
  refreshKey,
}: PaletteSelectorProps) {
  const [palettes, setPalettes] = useState<PaletteDto[]>([]);
  const lastCreatedId = useAppSelector((s) => s.palettes.lastCreatedId);

  useEffect(() => {
    listPalettes()
      .then((result) => {
        const sorted = [...result].sort((a, b) => a.id - b.id);
        setPalettes(sorted);
      })
      .catch(() => {});
  }, [refreshKey]);

  const handleChange = (value: string) => {
    if (value === 'none') {
      onChange(null);
    } else {
      onChange(Number(value));
    }
  };

  const effectiveId =
    selectedPaletteId == null && !allowNone && lastCreatedId != null
      ? lastCreatedId
      : selectedPaletteId;

  const currentValue = effectiveId === null ? 'none' : String(effectiveId);

  const options = [
    ...(allowNone ? [{ value: 'none', label: 'None (Uniform Quantization)' }] : []),
    ...palettes.map((p) => ({
      value: String(p.id),
      label: `${p.name} (${p.color_count} colors)`,
    })),
  ];

  return (
    <DropdownMenu
      label={label || undefined}
      value={currentValue}
      options={options}
      onSelect={handleChange}
    />
  );
}

export default PaletteSelector;
