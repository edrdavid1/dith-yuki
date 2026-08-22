// src/components/WindowControls.tsx
import React, { useCallback } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getPlatform } from '../lib/platform';

export function WindowControls() {
  // On macOS or unknown platform, native traffic lights handle window controls
  const platform = getPlatform();
  if (platform === 'macos') return null;

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
      const maximized = await win.isMaximized();
      if (maximized) {
        await win.unmaximize();
      } else {
        await win.maximize();
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

  return (
    <div
      className="window-controls"
      data-tauri-drag-region="false"
      style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
    >
      <button
        className="window-control-btn window-control-minimize"
        onClick={handleMinimize}
        title="Minimize"
        data-tauri-drag-region="false"
      >
        ─
      </button>
      <button
        className="window-control-btn window-control-maximize"
        onClick={handleMaximize}
        title="Maximize"
        data-tauri-drag-region="false"
      >
        □
      </button>
      <button
        className="window-control-btn window-control-close"
        onClick={handleClose}
        title="Close"
        data-tauri-drag-region="false"
      >
        ×
      </button>
    </div>
  );
}
