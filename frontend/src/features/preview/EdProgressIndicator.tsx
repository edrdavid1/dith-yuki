import { useEffect, useRef, useState } from 'react';
import {
  getEdProgress,
  setEdProgress,
  subscribeEdProgress,
  type EdProgressSnapshot,
} from '../../diagnostics/edProgress';
import styles from './EdProgressIndicator.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

const SEGMENT_COUNT = 12;
/** How many lit segments march in indeterminate (busy, no %) mode. */
const INDETERMINATE_WINDOW = 3;
const INDETERMINATE_TICK_MS = 90;
/** Hide progress for short bursts so the bar doesn’t flash. */
const SHOW_AFTER_MS = 200;

function edPct(snap: EdProgressSnapshot): number {
  if (snap.total <= 0) return 0;
  return Math.min(100, Math.round((snap.done / snap.total) * 100));
}

export interface EdProgressIndicatorProps {
  docId: number;
  /**
   * Preview recompute in flight (`full-document-busy`) — GPU families and CPU
   * tiled dither. Visibility is gated on this alone (plus the show delay).
   */
  busy?: boolean;
}

/**
 * Footer progress for any preview recompute:
 * - Determinate `%` while `ed-progress` reports done/total during busy
 * - Indeterminate marching segments while busy with no % yet
 * - Visible only after {@link SHOW_AFTER_MS} of continuous busy
 */
export default function EdProgressIndicator({
  docId,
  busy = false,
}: EdProgressIndicatorProps) {
  const [snap, setSnap] = useState<EdProgressSnapshot | null>(() => getEdProgress());
  const [march, setMarch] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const busySinceRef = useRef<number | null>(null);

  useEffect(() => subscribeEdProgress(setSnap), []);

  // Drop stale snap when the wave ends — incomplete viewport counts must not
  // keep the bar “active” and flash on the next interaction.
  useEffect(() => {
    if (!busy) {
      setEdProgress(null);
      setSnap(null);
      setRevealed(false);
      busySinceRef.current = null;
    }
  }, [busy]);

  useEffect(() => {
    if (!busy) return;
    if (busySinceRef.current == null) {
      busySinceRef.current = performance.now();
    }
    const started = busySinceRef.current;
    const remaining = Math.max(0, SHOW_AFTER_MS - (performance.now() - started));
    const id = window.setTimeout(() => {
      if (busySinceRef.current != null) setRevealed(true);
    }, remaining);
    return () => window.clearTimeout(id);
  }, [busy]);

  const show = busy && revealed;
  const hasPct =
    show && !!snap && snap.docId === docId && snap.total > 0;
  const indeterminate = show && !hasPct;

  useEffect(() => {
    if (!indeterminate) {
      setMarch(0);
      return;
    }
    const id = window.setInterval(() => {
      setMarch((m) => (m + 1) % SEGMENT_COUNT);
    }, INDETERMINATE_TICK_MS);
    return () => window.clearInterval(id);
  }, [indeterminate]);

  const pct = hasPct && snap ? edPct(snap) : 0;
  const filled = hasPct
    ? Math.max(0, Math.min(SEGMENT_COUNT, Math.round((pct / 100) * SEGMENT_COUNT)))
    : 0;

  const label = hasPct ? `${pct}%` : '…';

  return (
    <span
      className={cn('ed-progress', !show && 'ed-progress-idle')}
      role="progressbar"
      aria-valuenow={hasPct ? pct : undefined}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={
        !show
          ? 'Processing idle'
          : hasPct
            ? `Processing ${pct}%`
            : 'Processing'
      }
      aria-busy={show || undefined}
      aria-hidden={!show}
      title={
        !show
          ? undefined
          : hasPct && snap
            ? `${snap.sourceId} ${snap.done}/${snap.total}`
            : 'Preview recompute in progress'
      }
    >
      <span
        className={cn(
          'ed-progress-track',
          indeterminate && 'ed-progress-track-indeterminate',
        )}
        aria-hidden
      >
        {Array.from({ length: SEGMENT_COUNT }, (_, i) => {
          const on = hasPct
            ? i < filled
            : indeterminate &&
              ((i - march + SEGMENT_COUNT) % SEGMENT_COUNT) < INDETERMINATE_WINDOW;
          return (
            <span
              key={i}
              className={cn('ed-progress-seg', on && 'ed-progress-seg-on')}
            />
          );
        })}
      </span>
      <span className={cn('ed-progress-label')}>{label}</span>
    </span>
  );
}
