import { useEffect, useState } from 'react';
import {
  getEdProgress,
  subscribeEdProgress,
  type EdProgressSnapshot,
} from '../../diagnostics/edProgress';
import styles from './EdProgressIndicator.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

/**
 * Dithering progress for CPU ED (D1-a): done/total L0 Processed from `ed-progress`.
 */
export default function EdProgressIndicator({ docId }: { docId: number }) {
  const [snap, setSnap] = useState<EdProgressSnapshot | null>(() => getEdProgress());

  useEffect(() => subscribeEdProgress(setSnap), []);

  if (!snap || snap.docId !== docId || snap.total <= 0) return null;
  if (snap.done >= snap.total) return null;

  const pct = Math.min(100, Math.round((snap.done / snap.total) * 100));

  return (
    <span
      className={cn('ed-progress')}
      role="progressbar"
      aria-valuenow={pct}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={`Dithering ${pct}%`}
      title={`ED L0 ${snap.done}/${snap.total} · ${snap.sourceId}`}
    >
      <span className={cn('ed-progress-bar')} style={{ width: `${pct}%` }} />
      <span className={cn('ed-progress-label')}>Dither {pct}%</span>
    </span>
  );
}
