/**
 * Behavioral workspace hint banner (FR-9 / §6.3). Non-blocking; auto-dismiss 30s.
 */

import { useEffect } from 'react';
import { useWorkspace } from './WorkspaceContext';
import styles from './WorkspaceHintBanner.module.css';

const AUTO_MS = 30_000;

export default function WorkspaceHintBanner() {
  const { hintVisible, dismissHint, switchFromHint } = useWorkspace();

  useEffect(() => {
    if (!hintVisible) return;
    const t = window.setTimeout(() => {
      void dismissHint('timeout');
    }, AUTO_MS);
    return () => window.clearTimeout(t);
  }, [hintVisible, dismissHint]);

  if (!hintVisible) return null;

  return (
    <div className={styles.banner} role="status" data-testid="workspace-hint-banner">
      <p className={styles.text}>
        You&apos;re working with tiles. Enable Gamedev workspace for seamless
        preview and tiling tools.
      </p>
      <div className={styles.actions}>
        <button type="button" className={styles.btn} onClick={() => void switchFromHint()}>
          Switch
        </button>
        <button
          type="button"
          className={styles.btn}
          onClick={() => void dismissHint('dismiss')}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
