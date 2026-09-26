import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';
import { toHex } from '../../types/effects';
import { DialogTitlebar } from '../../shared/ui/WindowTitlebar';
import dialogStyles from '../document/NewProjectDialog.module.css';
import styles from './PaletteManagerDialog.module.css';
import { bind } from '../../shared/ui/cn';
import {
  buildPaletteList,
  currentPaletteKey,
  stepBrowseIndex,
  type PaletteListEntry,
} from './paletteListOrder';

const cn = bind({ ...dialogStyles, ...styles });

export interface PaletteManagerDialogProps {
  isOpen: boolean;
  onClose: () => void;
  builtins: BuiltinPaletteDto[];
  saved: PaletteDto[];
  selectedPaletteId: number | null;
  onSelectNew: () => void;
  onSelectSaved: (id: number) => void;
  onSelectBuiltin: (id: string) => void;
  onDeleteSaved: (id: number) => void;
  onExportSaved: (id: number) => void;
  onImport: () => void;
}

function Swatches({
  colors,
  swatchId,
}: {
  colors: [number, number, number][];
  swatchId: string;
}) {
  if (colors.length === 0) {
    return <span className={cn('pm-swatches', 'pm-swatches-empty')} aria-hidden />;
  }
  return (
    <span className={cn('pm-swatches')} aria-hidden>
      {colors.map(([r, g, b], i) => (
        <span
          key={`${swatchId}-${i}`}
          className={cn('pm-swatch')}
          style={{ backgroundColor: toHex(r, g, b) }}
        />
      ))}
    </span>
  );
}

export default function PaletteManagerDialog({
  isOpen,
  onClose,
  builtins,
  saved,
  selectedPaletteId,
  onSelectNew,
  onSelectSaved,
  onSelectBuiltin,
  onDeleteSaved,
  onExportSaved,
  onImport,
}: PaletteManagerDialogProps) {
  const entries = useMemo(() => buildPaletteList(builtins, saved), [builtins, saved]);
  const activeKey = currentPaletteKey(selectedPaletteId);
  const [focusKey, setFocusKey] = useState(activeKey);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isOpen) setFocusKey(activeKey);
  }, [isOpen, activeKey]);

  const focusIndex = useMemo(() => {
    const idx = entries.findIndex((e) => e.key === focusKey);
    return idx >= 0 ? idx : 0;
  }, [entries, focusKey]);

  const focused = entries[focusIndex] ?? null;

  useEffect(() => {
    if (!isOpen || !focused) return;
    const root = listRef.current;
    if (!root) return;
    const el = root.querySelector(`[data-pm-key="${focused.key.replace(/"/g, '\\"')}"]`);
    if (el instanceof HTMLElement && typeof el.scrollIntoView === 'function') {
      el.scrollIntoView({ block: 'nearest' });
    }
  }, [focused, isOpen]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  const stepFocus = useCallback(
    (delta: -1 | 1) => {
      if (entries.length === 0) return;
      const next = stepBrowseIndex(entries.length, focusIndex, delta);
      setFocusKey(entries[next]!.key);
    },
    [entries, focusIndex]
  );

  const applyEntry = useCallback(
    (entry: PaletteListEntry) => {
      if (entry.kind === 'new') onSelectNew();
      else if (entry.kind === 'builtin') onSelectBuiltin(entry.id);
      else onSelectSaved(entry.id);
      onClose();
    },
    [onClose, onSelectBuiltin, onSelectNew, onSelectSaved]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        stepFocus(1);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        stepFocus(-1);
        return;
      }
      if (e.key === 'Enter' && focused) {
        e.preventDefault();
        applyEntry(focused);
      }
    },
    [applyEntry, focused, onClose, stepFocus]
  );

  if (!isOpen) return null;

  const canDelete = focused?.kind === 'saved';
  const canExport = focused?.kind === 'saved';

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="palette-manager-overlay"
    >
      <div
        className={cn('new-project-dialog', 'pm-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="palette-manager-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar
          title="Palette Manager"
          titleId="palette-manager-title"
          onClose={onClose}
        />
        <div className={cn('pm-body')}>
          <SimpleBar className={cn('pm-list-scroll')}>
            <div className={cn('pm-list')} role="listbox" aria-label="Palettes" ref={listRef}>
              {entries.map((entry) => {
                const selected = entry.key === activeKey;
                const focusedRow = entry.key === focused?.key;
                return (
                  <button
                    key={entry.key}
                    type="button"
                    role="option"
                    data-pm-key={entry.key}
                    aria-selected={selected}
                    className={cn(
                      'pm-row',
                      selected && 'pm-row-selected',
                      focusedRow && 'pm-row-focused'
                    )}
                    onClick={() => setFocusKey(entry.key)}
                    onDoubleClick={() => applyEntry(entry)}
                  >
                    <Swatches
                      colors={entry.kind === 'new' ? [] : entry.colors}
                      swatchId={entry.key}
                    />
                    <span className={cn('pm-row-name')}>{entry.name}</span>
                    <span className={cn('pm-row-meta')}>
                      {entry.kind === 'builtin'
                        ? 'Built-in'
                        : entry.kind === 'saved'
                          ? 'Saved'
                          : 'Draft'}
                    </span>
                  </button>
                );
              })}
            </div>
          </SimpleBar>

          <div className={cn('pm-actions')}>
            <button
              type="button"
              className={cn('pm-action', 'pm-action-primary')}
              disabled={!focused}
              onClick={() => focused && applyEntry(focused)}
            >
              Select
            </button>
            <button
              type="button"
              className={cn('pm-action')}
              disabled={!canDelete}
              onClick={() => {
                if (focused?.kind === 'saved') onDeleteSaved(focused.id);
              }}
            >
              Delete
            </button>
            <button
              type="button"
              className={cn('pm-action')}
              disabled={!canExport}
              onClick={() => {
                if (focused?.kind === 'saved') onExportSaved(focused.id);
              }}
            >
              Export
            </button>
            <button type="button" className={cn('pm-action')} onClick={onImport}>
              Import
            </button>
          </div>
          <p className={cn('pm-hint')}>
            Arrow keys move the selection. Built-in palettes cannot be deleted. Double-click or
            Select to apply.
          </p>
        </div>
      </div>
    </div>,
    document.body
  );
}
