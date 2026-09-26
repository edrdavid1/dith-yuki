import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { refreshFilters } from '../../app/slices/filtersSlice';
import { refreshLayers } from '../../app/slices/layersSlice';
import { setSelection } from '../../app/slices/selectionSlice';
import {
  applyPatternFromLibrary,
  deletePatternFromLibrary,
  exportPatternFromLibrary,
  formatIpcError,
  importPatternToLibrary,
  listPatternLibrary,
  logIpcError,
  renamePatternInLibrary,
  savePatternToLibrary,
  type PatternLibraryEntry,
} from '../../shared/ipc';
import { openDialog, saveDialog } from '../../shared/ipc/dialogs';
import { DialogTitlebar } from '../../shared/ui/WindowTitlebar';
import overlayStyles from '../document/NewProjectDialog.module.css';
import styles from './PatternManagerDialog.module.css';
import { bind } from '../../shared/ui/cn';
import type { PatternManagerIntent } from './PatternsUiContext';

const cn = bind({ ...overlayStyles, ...styles });

export interface PatternManagerDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** browse = library; save = clear selection and focus Name for a new save. */
  intent?: PatternManagerIntent;
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
  return raw;
}

function isDyukiPath(path: string): boolean {
  return path.toLowerCase().endsWith('.dyuki');
}

export default function PatternManagerDialog({
  isOpen,
  onClose,
  intent = 'browse',
  intentNonce = 0,
}: PatternManagerDialogProps) {
  const dispatch = useAppDispatch();
  const docId = useAppSelector((s) => s.document.docId);
  const layers = useAppSelector((s) => s.layers.tree);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const imageSourceId = layers.length > 0 ? layers[0]!.id : null;
  const targetLayerId = selectedLayerId ?? imageSourceId;

  const [patterns, setPatterns] = useState<PatternLibraryEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [dropActive, setDropActive] = useState(false);
  const listZoneRef = useRef<HTMLDivElement>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);
  /** Paths from the last `enter` — `over` events do not include paths. */
  const dragPathsRef = useRef<string[]>([]);

  const selected = patterns.find((p) => p.id === selectedId) ?? null;
  const nameTrimmed = name.trim();
  const canRename =
    !!selected && !!nameTrimmed && nameTrimmed !== selected.name;
  const saveMode = intent === 'save';

  const refresh = useCallback(async () => {
    try {
      const list = await listPatternLibrary();
      setPatterns(list);
      setSelectedId((prev) => {
        if (prev && list.some((p) => p.id === prev)) return prev;
        return list[0]?.id ?? null;
      });
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.list', err);
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    setError(null);
    setDropActive(false);
    dragPathsRef.current = [];

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

  const importPaths = useCallback(
    async (paths: string[]) => {
      const dyuki = paths.filter(isDyukiPath);
      if (dyuki.length === 0) return;
      setBusy(true);
      setError(null);
      try {
        let lastId: string | null = null;
        for (const path of dyuki) {
          const entry = await importPatternToLibrary(path);
          lastId = entry.id;
        }
        await refresh();
        if (lastId) setSelectedId(lastId);
      } catch (err) {
        setError(formatIpcError(err));
        logIpcError('PatternManager.importDrop', err);
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;

        if (payload.type === 'leave') {
          dragPathsRef.current = [];
          setDropActive(false);
          return;
        }

        // Dialog is modal: any .dyuki drop into the window imports into the library.
        // List highlight follows any in-window .dyuki drag while the dialog is open.
        if (payload.type === 'drop') {
          dragPathsRef.current = [];
          setDropActive(false);
          void importPaths(payload.paths);
          return;
        }

        const hasDyuki =
          payload.type === 'enter'
            ? payload.paths.some(isDyukiPath)
            : dragPathsRef.current.some(isDyukiPath);

        if (payload.type === 'enter') {
          dragPathsRef.current = payload.paths;
        }

        setDropActive(hasDyuki);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* web / non-tauri */
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [importPaths, isOpen]);

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
    if (!selected) return;
    if (!canRename) return;
    setBusy(true);
    setError(null);
    try {
      await renamePatternInLibrary(selected.id, nameTrimmed);
      await refresh();
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.rename', err);
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async () => {
    if (docId == null || targetLayerId == null) {
      setError('Open a document and select a layer first.');
      return;
    }
    if (!nameTrimmed) {
      setError('Enter a name for the pattern.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const entry = await savePatternToLibrary({
        docId,
        layerId: targetLayerId,
        name: nameTrimmed,
      });
      await refresh();
      setSelectedId(entry.id);
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.save', err);
    } finally {
      setBusy(false);
    }
  };

  const handleApply = async (patternId?: string) => {
    const id = patternId ?? selected?.id;
    if (!id || docId == null || targetLayerId == null) return;
    setBusy(true);
    setError(null);
    try {
      const result = await applyPatternFromLibrary(docId, id, targetLayerId);
      await dispatch(refreshLayers(docId));
      await dispatch(refreshFilters());
      const last = result.filter_ids[result.filter_ids.length - 1] ?? null;
      if (last) {
        void dispatch(setSelection({ layerId: targetLayerId, filterId: last }));
      }
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.apply', err);
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await deletePatternFromLibrary(selected.id);
      await refresh();
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.delete', err);
    } finally {
      setBusy(false);
    }
  };

  const handleExport = async () => {
    if (!selected) return;
    try {
      const path = await saveDialog({
        filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
        defaultPath: `${selected.name.replace(/[^\w\-]+/g, '_') || 'pattern'}.dyuki`,
      } as Parameters<typeof saveDialog>[0]);
      if (!path) return;
      const out = path.toLowerCase().endsWith('.dyuki') ? path : `${path}.dyuki`;
      await exportPatternFromLibrary(selected.id, out);
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.export', err);
    }
  };

  const handleImport = async () => {
    try {
      const path = await openDialog({
        multiple: false,
        filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
      });
      if (!path || typeof path !== 'string') return;
      await importPaths([path]);
    } catch (err) {
      setError(formatIpcError(err));
      logIpcError('PatternManager.import', err);
    }
  };

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="pattern-manager-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide', 'pattern-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="pattern-manager-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Pattern Manager" titleId="pattern-manager-title" onClose={onClose} />
        <div className={cn('pattern-body')}>
          <div className={cn('pattern-columns')}>
            <div className={cn('pattern-list-col')}>
              <div className={cn('pattern-section-label')}>Library</div>
              <div
                ref={listZoneRef}
                className={cn('pattern-drop-zone', dropActive && 'pattern-drop-zone-active')}
                data-testid="pattern-drop-zone"
                aria-label="Pattern list. Drop .dyuki files to import."
              >
                <SimpleBar className={cn('pattern-list-scroll')}>
                  <div className={cn('pattern-list')} role="listbox" aria-label="Patterns">
                    {patterns.length === 0 && (
                      <div className={cn('pattern-empty')}>
                        Drop .dyuki files here, or save the current layer stack.
                      </div>
                    )}
                    {patterns.map((p) => (
                      <button
                        key={p.id}
                        type="button"
                        role="option"
                        aria-selected={p.id === selectedId}
                        className={cn('pattern-row', p.id === selectedId && 'pattern-row-selected')}
                        onClick={() => setSelectedId(p.id)}
                        onDoubleClick={() => void handleApply(p.id)}
                      >
                        <span className={cn('pattern-row-name')}>{p.name}</span>
                        <span className={cn('pattern-row-date')}>
                          {formatCreatedAt(p.created_at)}
                        </span>
                      </button>
                    ))}
                  </div>
                </SimpleBar>
                {dropActive && (
                  <div className={cn('pattern-drop-overlay')} aria-hidden>
                    Drop to import
                  </div>
                )}
              </div>
            </div>

            <div className={cn('pattern-detail-col')}>
              <label className={cn('pattern-field')}>
                <span>{saveMode ? 'Save as' : 'Name'}</span>
                <input
                  ref={nameInputRef}
                  type="text"
                  className={cn('pattern-input')}
                  value={name}
                  disabled={busy}
                  placeholder="Pattern name"
                  onChange={(e) => setName(e.target.value)}
                  onBlur={() => {
                    if (canRename) void commitName();
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      if (saveMode || !canRename) {
                        if (nameTrimmed) void handleSave();
                      } else {
                        void commitName();
                      }
                    }
                  }}
                />
              </label>
              <p className={cn('pattern-hint')}>
                {saveMode
                  ? 'Enter a name and press Save to store the active layer stack as a pattern.'
                  : 'Edit the name to rename the selected pattern. Save stores the active layer stack.'}
              </p>
              <div className={cn('pattern-btn-row')}>
                <button
                  type="button"
                  className={cn('pattern-btn', 'pattern-btn-primary')}
                  disabled={!selected || busy || docId == null || targetLayerId == null}
                  onClick={() => void handleApply()}
                >
                  Apply
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn', 'pattern-btn-primary', saveMode && 'pattern-btn-emphasis')}
                  disabled={busy || docId == null || targetLayerId == null || !nameTrimmed}
                  onClick={() => void handleSave()}
                >
                  Save
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn')}
                  disabled={!selected || busy}
                  onClick={() => void handleDelete()}
                >
                  Delete
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn')}
                  disabled={!selected || busy}
                  onClick={() => void handleExport()}
                >
                  Export
                </button>
                <button
                  type="button"
                  className={cn('pattern-btn')}
                  disabled={busy}
                  onClick={() => void handleImport()}
                >
                  Import
                </button>
              </div>
            </div>
          </div>
          {error && <div className={cn('pattern-error')}>{error}</div>}
        </div>
      </div>
    </div>,
    document.body
  );
}
