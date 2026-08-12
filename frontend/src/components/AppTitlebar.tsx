// src/components/AppTitlebar.tsx
import React, { useCallback } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { WindowControls } from './WindowControls';
import { isMacOS } from '../lib/platform';

interface AppTitlebarProps {
  children?: React.ReactNode;
  title?: string;
}

export function AppTitlebar({ children, title }: AppTitlebarProps) {
  // Double-click maximize/restore — only on Windows/Linux
  const handleDoubleClick = useCallback(async (e: React.MouseEvent) => {
    if (isMacOS()) return; // macOS handles this natively via -webkit-app-region

    // Ensure we're clicking the drag region, not a button or interactive element
    const target = e.target as HTMLElement;
    if (target.closest('button, input, [data-tauri-drag-region="false"]')) return;

    try {
      const win = getCurrentWindow();
      const maximized = await win.isMaximized();
      if (maximized) {
        await win.unmaximize();
      } else {
        await win.maximize();
      }
    } catch (err) {
      console.error('[AppTitlebar] double-click maximize toggle failed:', err);
    }
  }, []);

  const style: React.CSSProperties = isMacOS()
    ? ({ WebkitAppRegion: 'drag' } as unknown as React.CSSProperties)
    : {};

  return (
    <div
      className="app-titlebar"
      data-tauri-drag-region
      style={style}
      onDoubleClick={handleDoubleClick}
    >
      {title && <span className="app-titlebar-title" data-tauri-drag-region>{title}</span>}
      {children}
      <WindowControls />
    </div>
  );
}
