import { useCallback, useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppDispatch } from './hooks';
import { refreshFilters } from './slices/filtersSlice';
import { refreshLayers } from './slices/layersSlice';
import { refreshDocument } from './slices/documentSlice';
import { refreshTabs, tabsChanged } from './slices/tabsSlice';
import { allowAppExit, confirmAppQuit, onAppQuitRequested, onTabsChanged } from '../shared/ipc';
import { windowChromeTitle } from '../shared/windowTitle';
import type { Workspace } from '../shared/workspace/visibility';

type LifetimeDoc = {
  dirty: boolean;
  hasDocument: boolean;
  projectPath: string | null;
  sourcePath: string | null;
  docId: number | null;
};

/**
 * Main-window title, tab refresh, and the quit/close path.
 * Close flushes the shell layout before the window is destroyed.
 */
export function useMainWindowLifetime(
  doc: LifetimeDoc,
  workspace: Workspace,
  confirmQuit: () => Promise<boolean>,
): { requestQuit: () => Promise<void> } {
  const dispatch = useAppDispatch();
  const allowCloseRef = useRef(false);
  const quitInFlightRef = useRef(false);
  const confirmQuitRef = useRef(confirmQuit);
  confirmQuitRef.current = confirmQuit;

  const requestQuit = useCallback(async () => {
    if (quitInFlightRef.current) return;
    quitInFlightRef.current = true;
    try {
      const ok = await confirmQuitRef.current();
      if (!ok) return;
      allowCloseRef.current = true;
      await confirmAppQuit();
    } catch (err) {
      console.error('Quit failed:', err);
    } finally {
      quitInFlightRef.current = false;
    }
  }, []);

  useEffect(() => {
    const title = windowChromeTitle({
      dirty: doc.dirty,
      hasDocument: doc.hasDocument,
      projectPath: doc.projectPath,
      sourcePath: doc.sourcePath,
      workspace,
    });
    void getCurrentWindow()
      .setTitle(title)
      .catch(() => {
        /* panel webviews may lack set-title; main title is enough */
      });
  }, [doc.dirty, doc.hasDocument, doc.projectPath, doc.sourcePath, workspace]);

  useEffect(() => {
    void dispatch(refreshTabs());
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void onTabsChanged((event) => {
      dispatch(tabsChanged(event.payload));
      void dispatch(refreshDocument());
      void dispatch(refreshLayers(event.payload.active_id));
      void dispatch(refreshFilters());
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [dispatch]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    const win = getCurrentWindow();
    void win
      .onCloseRequested(async (event) => {
        if (allowCloseRef.current) return;
        event.preventDefault();
        const ok = await confirmQuitRef.current();
        if (!ok) return;
        allowCloseRef.current = true;
        // W7: flush shell layout before destroy (also handles flush-layout from Rust).
        try {
          const { layoutStore } = await import('../contexts/layoutStore');
          await layoutStore.flushForClose();
        } catch (err) {
          console.error('[flex-dock] close flush failed:', err);
        }
        try {
          await allowAppExit();
        } catch {
          /* still try to close */
        }
        await win.destroy();
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void onAppQuitRequested(() => {
      void requestQuit();
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [requestQuit]);

  useEffect(() => {
    void dispatch(refreshLayers(doc.docId));
    if (doc.docId !== null) {
      void dispatch(refreshFilters());
    }
  }, [dispatch, doc.docId]);

  return { requestQuit };
}
