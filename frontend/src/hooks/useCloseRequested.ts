import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { dockPanel } from '../ipc/panelCommands';

/**
 * Registers an onCloseRequested listener that intercepts window close,
 * docks the panel, then destroys the window.
 */
export function useCloseRequested(panelId: string) {
  useEffect(() => {
    let cancelled = false;
    const win = getCurrentWindow();

    const setupListener = async () => {
      const unlisten = await win.onCloseRequested(async (event) => {
        if (cancelled) return;
        event.preventDefault();

        try {
          await dockPanel(panelId);
        } catch (err) {
          console.error(`[useCloseRequested] dock_panel failed for "${panelId}":`, err);
        }

        await win.destroy();
      });

      return unlisten;
    };

    let unlistenFn: (() => void) | null = null;
    setupListener().then((fn) => {
      if (cancelled) { fn(); } else { unlistenFn = fn; }
    });

    return () => {
      cancelled = true;
      if (unlistenFn) unlistenFn();
    };
  }, [panelId]);
}
