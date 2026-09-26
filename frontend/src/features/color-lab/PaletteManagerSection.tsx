import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import DropdownMenu, { type DropdownOption } from '../../components/common/DropdownMenu';
import Icon from '../../icons/iconRegistry';
import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';
import { toHex } from '../../types/effects';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';
import PaletteManagerDialog from './PaletteManagerDialog';
import {
  PALETTE_NEW_VALUE,
  buildBrowsablePaletteList,
  browseIndexForSelection,
  builtinKey,
  currentPaletteKey,
  savedKey,
  stepBrowseIndex,
  type PaletteListEntry,
} from './paletteListOrder';

const cn = bind({ ...styles, ...buttonStyles });

export { PALETTE_NEW_VALUE, builtinKey, savedKey };

export interface PaletteManagerSectionProps {
  builtins: BuiltinPaletteDto[];
  saved: PaletteDto[];
  selectedPaletteId: number | null;
  onSelectNew: () => void;
  onSelectSaved: (id: number) => void;
  onSelectBuiltin: (id: string) => void;
  onDeleteSaved: (id: number) => void;
  onExportSaved: (id: number) => void;
  onImport: () => void;
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
 * Palette picker + ◀ / Manager / ▶ — shared list order with Palette Manager.
 */
export default function PaletteManagerSection({
  builtins,
  saved,
  selectedPaletteId,
  onSelectNew,
  onSelectSaved,
  onSelectBuiltin,
  onDeleteSaved,
  onExportSaved,
  onImport,
  previewColors,
}: PaletteManagerSectionProps) {
  const [managerOpen, setManagerOpen] = useState(false);
  const value = currentPaletteKey(selectedPaletteId);
  const selectedSaved = saved.find((p) => p.id === selectedPaletteId) ?? null;
  const browseList = useMemo(
    () => buildBrowsablePaletteList(builtins, saved),
    [builtins, saved]
  );

  /** Sticky browse cursor so ◀▶ walks builtins even after import creates a saved copy. */
  const browseIndexRef = useRef<number | null>(null);
  const [browseIndex, setBrowseIndex] = useState(0);

  useEffect(() => {
    const idx = browseIndexForSelection(
      browseList,
      selectedPaletteId,
      saved,
      browseIndexRef.current
    );
    setBrowseIndex(idx);
  }, [browseList, selectedPaletteId, saved]);

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

  const applyEntry = useCallback(
    (entry: PaletteListEntry | null | undefined) => {
      if (!entry) return;
      if (entry.kind === 'new') onSelectNew();
      else if (entry.kind === 'builtin') onSelectBuiltin(entry.id);
      else onSelectSaved(entry.id);
    },
    [onSelectBuiltin, onSelectNew, onSelectSaved]
  );

  const handleStep = useCallback(
    (delta: -1 | 1) => {
      if (browseList.length === 0) return;
      // From draft (New): first ◀/▶ lands on an end of the browsable list.
      const next =
        selectedPaletteId == null
          ? delta === 1
            ? 0
            : browseList.length - 1
          : stepBrowseIndex(browseList.length, browseIndex, delta);
      browseIndexRef.current = next;
      setBrowseIndex(next);
      applyEntry(browseList[next]);
    },
    [applyEntry, browseIndex, browseList, selectedPaletteId]
  );

  const handleDropdownSelect = useCallback(
    (v: string) => {
      browseIndexRef.current = null;
      if (v === PALETTE_NEW_VALUE) {
        onSelectNew();
        return;
      }
      if (v.startsWith('builtin:')) {
        const id = v.slice('builtin:'.length);
        const idx = browseList.findIndex((e) => e.kind === 'builtin' && e.id === id);
        if (idx >= 0) browseIndexRef.current = idx;
        onSelectBuiltin(id);
        return;
      }
      if (v.startsWith('saved:')) {
        const id = Number(v.slice('saved:'.length));
        if (!Number.isFinite(id)) return;
        const idx = browseList.findIndex((e) => e.kind === 'saved' && e.id === id);
        if (idx >= 0) browseIndexRef.current = idx;
        onSelectSaved(id);
      }
    },
    [browseList, onSelectBuiltin, onSelectNew, onSelectSaved]
  );

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
        onSelect={handleDropdownSelect}
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

      <div className={cn('palette-nav-row')}>
        <button
          type="button"
          className={cn('color-lab-button', 'palette-nav-btn')}
          aria-label="Previous palette"
          title="Previous palette"
          onClick={() => handleStep(-1)}
          disabled={browseList.length < 2}
        >
          <Icon name="arrow-left" width={14} height={14} />
        </button>
        <button
          type="button"
          className={cn('color-lab-button', 'palette-nav-manager')}
          aria-label="Open palette manager"
          onClick={() => setManagerOpen(true)}
        >
          Manager
        </button>
        <button
          type="button"
          className={cn('color-lab-button', 'palette-nav-btn')}
          aria-label="Next palette"
          title="Next palette"
          onClick={() => handleStep(1)}
          disabled={browseList.length < 2}
        >
          <Icon name="arrow-right" width={14} height={14} />
        </button>
      </div>

      <PaletteManagerDialog
        isOpen={managerOpen}
        onClose={() => setManagerOpen(false)}
        builtins={builtins}
        saved={saved}
        selectedPaletteId={selectedPaletteId}
        onSelectNew={onSelectNew}
        onSelectSaved={onSelectSaved}
        onSelectBuiltin={onSelectBuiltin}
        onDeleteSaved={onDeleteSaved}
        onExportSaved={onExportSaved}
        onImport={onImport}
      />
    </div>
  );
}
