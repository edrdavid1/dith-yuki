import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { refreshFilters } from '../../app/slices/filtersSlice';
import { refreshLayers } from '../../app/slices/layersSlice';
import {
  formatIpcError,
  logIpcError,
  presetApplyToDoc,
  presetDelete,
  presetList,
  presetRename,
  presetSave,
  type PresetLibraryEntry,
} from '../../shared/ipc';
import { DialogTitlebar } from '../../shared/ui/WindowTitlebar';
import overlayStyles from '../document/NewProjectDialog.module.css';
import patternStyles from '../patterns/PatternManagerDialog.module.css';
import { bind } from '../../shared/ui/cn';
import type { PresetManagerIntent } from './PresetsUiContext';

const cn = bind({ ...overlayStyles, ...patternStyles });

export interface PresetManagerDialogProps {
  isOpen: boolean;
  onClose: () => void;
  intent?: PresetManagerIntent;
  intentNonce?: number;
}

function formatCreatedAt(raw: string): string {
  if (!raw) return '—';
  const asDate = Date.parse(raw);
  if (Number.isFinite(asDate)) {
    try {
      return new Date(asDate).toLocaleString();
    } catch {
      /* fall through */
    }
  }
  // Unix-seconds stamp from backend when ISO unavailable.
  const secs = Number(raw);
  if (Number.isFinite(secs) && secs > 1e9) {
    try {
      return new Date(secs * 1000).toLocaleString();
    } catch {
      /* fall through */
    }
  }
  return raw;
}

export default function PresetManagerDialog({
  isOpen,
  onClose,
  intent = 'browse',
  intentNonce = 0,
}: PresetManagerDialogProps) {
  const dispatch = useAppDispatch();
  const docId = useAppSelector((s) => s.document.docId);
  const layers = useAppSelector((s) => s.layers.tree);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const imageSourceId = layers.length > 0 ? layers[0]!.id : null;
  const targetLayerId = selectedLayerId ?? imageSourceId;

  const [presets, setPresets] = useState<PresetLibraryEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const nameInputRef = useRef<HTMLInputElement>(null);

  const selected = presets.find((p) => p.id === selectedId) ?? null;
  const nameTrimmed = name.trim();
  const canRename = !!selected && !!nameTrimmed && nameTrimmed !== selected.name;
  const saveMode = intent === 'save';

  const refresh = useCallback(async () => {
    try {
      const list = await presetList();
      setPresets(list);
      setSelectedId((prev) => {
        if (prev && list.some((p) => p.id === prev)) return prev;
        return list[0]?.id ?? null;
      });
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PresetManager.list', err);
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    setError(null);
    setWarnings([]);
    void (async () => {
      await refresh();
      if (cancelled) return;
      if (intent === 'save') {
        setSelectedId(null);
        setName('');
        requestAnimationFrame(() => {
          nameInputRef.current?.focus();
          nameInputRef.current?.select();
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [isOpen, intent, intentNonce, refresh]);

  useEffect(() => {
    if (intent === 'save' && selectedId == null) return;
    setName(selected?.name ?? '');
  }, [selected, selectedId, intent]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [onClose]
  );

  const commitName = async () => {
    if (!selected || !canRename) return;
    setBusy(true);
    setError(null);
    try {
      await presetRename(selected.id, nameTrimmed);
      await refresh();
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PresetManager.rename', err);
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async () => {
    if (docId == null) {
      setError('Open a document first.');
      return;
    }
    if (!nameTrimmed) {
      setError('Enter a name for the preset.');
      return;
    }
    setBusy(true);
    setError(null);
    setWarnings([]);
    try {
      const entry = await presetSave({
        docId,
        layerId: targetLayerId,
        name: nameTrimmed,
      });
      await refresh();
      setSelectedId(entry.id);
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PresetManager.save', err);
    } finally {
      setBusy(false);
    }
  };

  const handleApply = async () => {
    if (!selected || docId == null) return;
    setBusy(true);
    setError(null);
    setWarnings([]);
    try {
      const result = await presetApplyToDoc(docId, selected.id, targetLayerId);
      await dispatch(refreshLayers(docId));
      await dispatch(refreshFilters());
      if (result.warnings.length > 0) {
        setWarnings(result.warnings);
      }
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PresetManager.apply', err);
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await presetDelete(selected.id);
      await refresh();
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PresetManager.delete', err);
    } finally {
      setBusy(false);
    }
  };

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="preset-manager-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide', 'pattern-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="preset-manager-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Preset Manager" titleId="preset-manager-title" onClose={onClose} />
        <div className={cn('pattern-body')}>
          <p className={cn('pattern-hint')}>
            Saves the filter stack (algorithms, params, order) and palette bindings by id.
            Export format and paths stay per-export.
          </p>
          <div className={cn('pattern-columns')}>
            <div className={cn('pattern-list-col')}>
              <div className={cn('pattern-section-label')}>Presets</div>
              <div className={cn('pattern-list-scroll')}>
                <SimpleBar style={{ maxHeight: '100%' }}>
                  {presets.length === 0 ? (
                    <p className={cn('pattern-hint')} style={{ padding: 8 }}>
                      No presets yet. Save the current stack to create one.
                    </p>
                  ) : (
                    <ul className={cn('pattern-list')} role="listbox">
                      {presets.map((p) => (
                        <li key={p.id}>
                          <button
                            type="button"
                            className={cn(
                              'pattern-row',
                              selectedId === p.id && 'pattern-row-selected'
                            )}
                            onClick={() => setSelectedId(p.id)}
                          >
                            <span className={cn('pattern-row-name')}>{p.name}</span>
                            <span className={cn('pattern-row-date')}>
                              {p.filter_count} filter{p.filter_count === 1 ? '' : 's'} ·{' '}
                              {formatCreatedAt(p.created_at)}
                            </span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </SimpleBar>
              </div>
            </div>
            <div className={cn('pattern-detail-col')}>
              <div className={cn('pattern-section-label')}>
                {saveMode ? 'Save current stack' : 'Details'}
              </div>
              <label className={cn('pattern-field')}>
                <span>Name</span>
                <input
                  ref={nameInputRef}
                  className={cn('pattern-input')}
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  disabled={busy}
                  maxLength={255}
                />
              </label>
              {error && (
                <p className={cn('pattern-error')} role="alert">
                  {error}
                </p>
              )}
              {warnings.length > 0 && (
                <ul className={cn('pattern-hint')} role="status">
                  {warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              )}
              <div className={cn('pattern-btn-row')}>
                <button
                  type="button"
                  className={cn('pattern-btn', 'pattern-btn-primary')}
                  disabled={busy || !nameTrimmed || docId == null}
                  onClick={() => void handleSave()}
                >
                  Save from stack
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn')}
                  disabled={busy || !canRename}
                  onClick={() => void commitName()}
                >
                  Rename
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn', 'pattern-btn-emphasis')}
                  disabled={busy || !selected || docId == null}
                  onClick={() => void handleApply()}
                >
                  Apply
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn')}
                  disabled={busy || !selected}
                  onClick={() => void handleDelete()}
                >
                  Delete
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
