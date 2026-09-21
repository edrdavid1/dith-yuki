import React, { useCallback, useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getPlatform } from '../lib/platform';

/**
 * Custom caption buttons for Windows/Linux (frameless main window).
 * macOS keeps native traffic lights.
 */
export function WindowControls() {
  const platform = getPlatform();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (platform === 'macos') return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const sync = async () => {
      try {
        const next = await getCurrentWindow().isMaximized();
        if (!cancelled) setMaximized(next);
      } catch {
        /* ignore */
      }
    };

    void sync();
    void getCurrentWindow()
      .onResized(() => {
        void sync();
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* ignore */
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [platform]);

  const handleMinimize = useCallback(async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (err) {
      console.error('[WindowControls] minimize failed:', err);
    }
  }, []);

  const handleMaximize = useCallback(async () => {
    try {
      const win = getCurrentWindow();
      const isMax = await win.isMaximized();
      if (isMax) {
        await win.unmaximize();
        setMaximized(false);
      } else {
        await win.maximize();
        setMaximized(true);
      }
    } catch (err) {
      console.error('[WindowControls] maximize toggle failed:', err);
    }
  }, []);

  const handleClose = useCallback(async () => {
    try {
      await getCurrentWindow().close();
    } catch (err) {
      console.error('[WindowControls] close failed:', err);
    }
  }, []);

  if (platform === 'macos') return null;

  const maximizeLabel = maximized ? 'Restore' : 'Maximize';

  return (
    <div
      className="window-controls"
      data-tauri-drag-region="false"
      style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
    >
      <button
        type="button"
        className="window-control-btn window-control-minimize"
        onClick={() => void handleMinimize()}
        title="Minimize"
        aria-label="Minimize"
        data-tauri-drag-region="false"
      >
        <img src="/icons/hide-window-icon.svg" width="14" height="14" alt="" />
      </button>
      <button
        type="button"
        className="window-control-btn window-control-maximize"
        onClick={() => void handleMaximize()}
        title={maximizeLabel}
        aria-label={maximizeLabel}
        data-tauri-drag-region="false"
      >
        <img src="/icons/header-window-square-icon.svg" width="14" height="14" alt="" />
      </button>
      <button
        type="button"
        className="window-control-btn window-control-close"
        onClick={() => void handleClose()}
        title="Close"
        aria-label="Close"
        data-tauri-drag-region="false"
      >
        <img src="/icons/clouse-window-icon.svg" width="14" height="14" alt="" />
      </button>
    </div>
  );
}
