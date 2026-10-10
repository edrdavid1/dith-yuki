import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import { openDialog } from '../shared/ipc/dialogs';
import {
  batchExportCancel,
  batchExportRun,
  onBatchExportProgress,
  type BatchExportProgress,
  type BatchExportSummary,
  type BatchOutputFormat,
} from '../shared/ipc/batchExport';

const cn = bind(styles);

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

  useEffect(() => {
    if (!isOpen) return;
    setProgress(null);
    setError(null);
    setRunning(false);
  }, [isOpen]);

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
      setError(String(e));
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

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="batch-export-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide')}
        role="dialog"
        aria-modal="true"
        aria-label="Batch Export"
        style={{ width: 420 }}
      >
        <DialogTitlebar title="Batch Export" onClose={handleCancel} />
        <div className={cn('new-project-body')} style={{ padding: '14px 12px 12px' }}>
          <p style={{ margin: 0, fontSize: 12, lineHeight: 1.4 }}>
            Apply this document’s filter stack and palettes to every image in a folder.
          </p>

          <div className={cn('new-project-field')}>
            <label htmlFor="batch-input-dir">Input folder</label>
            <div style={{ display: 'flex', gap: 6 }}>
              <input
                id="batch-input-dir"
                className={cn('new-project-input')}
                style={{ flex: 1 }}
                value={inputDir}
                readOnly
                placeholder="Choose a folder…"
              />
              <button
                type="button"
                className={cn('new-project-btn')}
                onClick={() => void pickInput()}
                disabled={running}
              >
                Browse…
              </button>
            </div>
          </div>

          <div className={cn('new-project-field')}>
            <label htmlFor="batch-output-dir">Output folder</label>
            <div style={{ display: 'flex', gap: 6 }}>
              <input
                id="batch-output-dir"
                className={cn('new-project-input')}
                style={{ flex: 1 }}
                value={outputDir}
                readOnly
                placeholder="Choose a folder…"
              />
              <button
                type="button"
                className={cn('new-project-btn')}
                onClick={() => void pickOutput()}
                disabled={running}
              >
                Browse…
              </button>
            </div>
          </div>

          <div className={cn('new-project-field')}>
            <label htmlFor="batch-name-template">Filename template</label>
            <input
              id="batch-name-template"
              className={cn('new-project-input')}
              value={nameTemplate}
              onChange={(e) => setNameTemplate(e.target.value)}
              disabled={running}
            />
            <span style={{ fontSize: 11, opacity: 0.7 }}>
              Use {'{name}'} and {'{ext}'} from the source file.
            </span>
          </div>

          <fieldset className={cn('new-project-field')}>
            <legend>Format</legend>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="batch-format"
                checked={format === 'png'}
                onChange={() => setFormat('png')}
                disabled={running}
              />
              PNG RGBA
            </label>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="batch-format"
                checked={format === 'png8'}
                onChange={() => setFormat('png8')}
                disabled={running}
              />
              PNG8 (indexed)
            </label>
          </fieldset>

          <label className={cn('new-project-radio')}>
            <input
              type="checkbox"
              checked={lockPhase}
              onChange={(e) => setLockPhase(e.target.checked)}
              disabled={running}
            />
            Lock pattern phase (stable dither across frames)
          </label>

          {progressLabel && (
            <div style={{ fontSize: 12 }}>
              Progress: {progressLabel}
              {progress?.current_input
                ? ` — ${progress.current_input.split(/[/\\]/).pop()}`
                : ''}
              {progress?.last_error ? (
                <div style={{ color: 'var(--color-danger, #a00)', marginTop: 4 }}>
                  Last error: {progress.last_error}
                </div>
              ) : null}
            </div>
          )}

          {error && (
            <div style={{ fontSize: 12, color: 'var(--color-danger, #a00)' }}>{error}</div>
          )}

          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={handleCancel}>
              {running ? 'Cancel' : 'Close'}
            </button>
            <button
              type="button"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              onClick={() => void handleStart()}
              disabled={running || !inputDir || !outputDir || docId == null}
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
