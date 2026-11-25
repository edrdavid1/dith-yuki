import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import overlayStyles from '../features/document/NewProjectDialog.module.css';
import styles from './BatchExportDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import InterfaceHint from '../shared/ui/InterfaceHint';
import { openDialog } from '../shared/ipc/dialogs';
import {
  batchExportCancel,
  batchExportRun,
  onBatchExportProgress,
  type BatchExportProgress,
  type BatchExportSummary,
  type BatchOutputFormat,
} from '../shared/ipc/batchExport';
import {
  EXPORT_JOB_COPY,
  jobBusyMessage,
  parseJobBusyError,
  useExportJobBusy,
} from '../shared/exportJobGate';
import { usePatternsUi } from '../features/patterns/PatternsUiContext';
import { useWorkspaceOptional } from '../features/workspace/WorkspaceContext';
import { useGamedevStoreOptional } from '../features/workspace/GamedevContext';

const cn = bind({ ...overlayStyles, ...styles });

export interface BatchExportDialogProps {
  isOpen: boolean;
  docId: number | null;
  layerId: number | null;
  onClose: () => void;
  onDone: (summary: BatchExportSummary) => void;
}

export default function BatchExportDialog({
  isOpen,
  docId,
  layerId,
  onClose,
  onDone,
}: BatchExportDialogProps) {
  const [inputDir, setInputDir] = useState('');
  const [outputDir, setOutputDir] = useState('');
  const [nameTemplate, setNameTemplate] = useState('{name}_dithered.png');
  const [format, setFormat] = useState<BatchOutputFormat>('png');
  const [lockPhase, setLockPhase] = useState(true);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<BatchExportProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { openPatternManager } = usePatternsUi();
  const workspace = useWorkspaceOptional();
  const gamedev = useGamedevStoreOptional();
  const exportBusy = useExportJobBusy();
  const blockedByOtherJob = exportBusy === 'print';

  const maskFixed =
    gamedev?.context.threshold_mask != null && gamedev.context.threshold_mask !== 'free';

  useEffect(() => {
    if (!isOpen) return;
    setProgress(null);
    setError(null);
    setRunning(false);
    // Prefill naming / PNG8 / lock phase from GamedevContext (Tile + Sprites slice).
    const naming = gamedev?.context.export.naming;
    if (naming) setNameTemplate(naming);
    if (gamedev?.context.export.png8) setFormat('png8');
    if (maskFixed) setLockPhase(true);
    // FR-9: Batch Export in General suggests Gamedev (tile/atlas workflows).
    void workspace?.triggerHint({ trigger: 'batch_tile' });
  }, [
    isOpen,
    workspace,
    gamedev?.context.export.naming,
    gamedev?.context.export.png8,
    maskFixed,
  ]);

  useEffect(() => {
    if (!isOpen || !running) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onBatchExportProgress((p) => {
      if (!cancelled) setProgress(p);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [isOpen, running]);

  const pickInput = useCallback(async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === 'string') setInputDir(dir);
  }, []);

  const pickOutput = useCallback(async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === 'string') setOutputDir(dir);
  }, []);

  const handleCancel = useCallback(() => {
    if (running) {
      void batchExportCancel();
      return;
    }
    onClose();
  }, [running, onClose]);

  const handleStart = useCallback(async () => {
    if (docId == null || !inputDir || !outputDir) return;
    setRunning(true);
    setError(null);
    setProgress({ done: 0, total: 0, current_input: '', last_error: null, stage: 'running' });
    try {
      const summary = await batchExportRun({
        doc_id: docId,
        layer_id: layerId,
        input_dir: inputDir,
        output_dir: outputDir,
        name_template: nameTemplate,
        format,
        lock_pattern_phase: lockPhase,
      });
      onDone(summary);
    } catch (e) {
      const busy = parseJobBusyError(e);
      setError(busy ? jobBusyMessage(busy) : String(e));
    } finally {
      setRunning(false);
    }
  }, [docId, layerId, inputDir, outputDir, nameTemplate, format, lockPhase, onDone]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget && !running) onClose();
    },
    [onClose, running]
  );

  if (!isOpen) return null;

  const progressLabel =
    progress && progress.total > 0
      ? `${progress.done} / ${progress.total}`
      : running
        ? 'Starting…'
        : null;
  const progressPct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : running
        ? 0
        : null;
  const currentFile = progress?.current_input
    ? progress.current_input.split(/[/\\]/).pop()
    : null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="batch-export-overlay"
    >
      <div
        className={cn('new-project-dialog', 'batch-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="batch-export-title"
      >
        <DialogTitlebar
          title="Batch Export"
          titleId="batch-export-title"
          onClose={handleCancel}
        />
        <div className={cn('batch-body')}>
          <InterfaceHint>
            <p className={cn('batch-intro')}>
              Apply this document’s filter stack and palettes to every image in a folder. Save or
              reuse a named recipe via Pattern Manager (export format stays here).
            </p>
          </InterfaceHint>

          <button
            type="button"
            className={cn('batch-btn', 'batch-btn-block')}
            disabled={running}
            onClick={() => openPatternManager('browse')}
          >
            Pattern Manager…
          </button>

          <div className={cn('batch-field')}>
            <label htmlFor="batch-input-dir">Input folder</label>
            <div className={cn('batch-path-row')}>
              <input
                id="batch-input-dir"
                className={cn('batch-input')}
                value={inputDir}
                readOnly
                placeholder="Choose a folder…"
              />
              <button
                type="button"
                className={cn('batch-btn')}
                onClick={() => void pickInput()}
                disabled={running}
              >
                Browse…
              </button>
            </div>
          </div>

          <div className={cn('batch-field')}>
            <label htmlFor="batch-output-dir">Output folder</label>
            <div className={cn('batch-path-row')}>
              <input
                id="batch-output-dir"
                className={cn('batch-input')}
                value={outputDir}
                readOnly
                placeholder="Choose a folder…"
              />
              <button
                type="button"
                className={cn('batch-btn')}
                onClick={() => void pickOutput()}
                disabled={running}
              >
                Browse…
              </button>
            </div>
          </div>

          <div className={cn('batch-field')}>
            <label htmlFor="batch-name-template">Filename template</label>
            <input
              id="batch-name-template"
              className={cn('batch-input')}
              value={nameTemplate}
              onChange={(e) => setNameTemplate(e.target.value)}
              disabled={running}
            />
            <InterfaceHint>
              <p className={cn('batch-hint')}>
                Use {'{name}'} and {'{ext}'} from the source file.
              </p>
            </InterfaceHint>
          </div>

          <fieldset className={cn('batch-field')}>
            <legend>Format</legend>
            <div className={cn('batch-radio-stack')}>
              <label className={cn('batch-radio')}>
                <input
                  type="radio"
                  name="batch-format"
                  checked={format === 'png'}
                  onChange={() => setFormat('png')}
                  disabled={running}
                />
                PNG RGBA
              </label>
              <label className={cn('batch-radio')}>
                <input
                  type="radio"
                  name="batch-format"
                  checked={format === 'png8'}
                  onChange={() => setFormat('png8')}
                  disabled={running}
                />
                PNG8 (indexed)
              </label>
            </div>
          </fieldset>

          <label
            className={cn('batch-check')}
            title={
              maskFixed
                ? 'Pattern phase is locked from Gamedev Settings so every file matches.'
                : 'Keep the ordered / blue-noise phase identical across every file.'
            }
          >
            <input
              type="checkbox"
              checked={lockPhase}
              onChange={(e) => setLockPhase(e.target.checked)}
              disabled={running || maskFixed}
            />
            {maskFixed
              ? 'Pattern phase locked (Gamedev)'
              : 'Lock pattern phase (stable dither across frames)'}
          </label>

          {progressLabel && (
            <div className={cn('batch-progress')}>
              {progressPct != null && (
                <div className={cn('batch-progress-track')} aria-hidden>
                  <div
                    className={cn('batch-progress-fill')}
                    style={{ width: `${progressPct}%` }}
                  />
                </div>
              )}
              <div className={cn('batch-progress-meta')}>
                Progress: {progressLabel}
                {currentFile ? ` — ${currentFile}` : ''}
              </div>
              {progress?.last_error ? (
                <p className={cn('batch-warn')}>Last error: {progress.last_error}</p>
              ) : null}
            </div>
          )}

          {error && <p className={cn('batch-error')}>{error}</p>}

          <div className={cn('batch-footer')}>
            <button type="button" className={cn('batch-btn')} onClick={handleCancel}>
              {running ? 'Cancel' : 'Close'}
            </button>
            <button
              type="button"
              className={cn('batch-btn', 'batch-btn-primary')}
              onClick={() => void handleStart()}
              disabled={
                running || blockedByOtherJob || !inputDir || !outputDir || docId == null
              }
              title={blockedByOtherJob ? EXPORT_JOB_COPY.busyTooltip : undefined}
            >
              {running ? 'Exporting…' : 'Start'}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
