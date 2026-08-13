import React, { useEffect, useRef, useCallback, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  beginFloatDrag,
  cancelFloatDrag,
  dockPanel,
  onDockAffinity,
  savePanelBounds,
} from '../shared/ipc';
import { FLOATING_ONLY_PANELS, PANEL_DISPLAY_NAMES } from '../types/panels';
import type { PanelId, PanelInfo, PanelStateSnapshot } from '../types/panels';
import { useCloseRequested } from '../hooks/useCloseRequested';
import ColorLabFeature from '../features/color-lab/ColorLabFeature';
import EffectsFeature from '../features/effects/EffectsFeature';
import LayersFeature from '../features/layers/LayersFeature';
import PreviewFeature from '../features/preview/PreviewFeature';
import PreferencesFeature from '../features/preferences/PreferencesFeature';
import NewProjectDialog from './NewProjectDialog';
import { useWelcomeScreen } from '../hooks/useWelcomeScreen';
import styles from '../features/panels/PanelWindow.module.css';
import { bind } from '../shared/ui/cn';
const cn = bind(styles);

interface PanelWindowProps {
  panelId: string;
}

function FloatingPreview(): JSX.Element {
  const { welcome, newProjectOpen, closeNewProject, handleCreate } = useWelcomeScreen();
  return (
    <>
      <PreviewFeature hideTitleBar fill welcome={welcome} />
      <NewProjectDialog
        isOpen={newProjectOpen}
        onClose={closeNewProject}
        onCreate={handleCreate}
      />
    </>
  );
}

/**
 * Floating panel window shell — reuses the same feature containers as the main app.
 */
function PanelWindow({ panelId }: PanelWindowProps): JSX.Element {
  const bounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const displayName = PANEL_DISPLAY_NAMES[panelId as PanelId] ?? panelId;
  const [affinityArmed, setAffinityArmed] = useState(false);
  const canRedock = !FLOATING_ONLY_PANELS.has(panelId as PanelId);

  useCloseRequested(panelId);

  const handleClose = useCallback(async () => {
    try {
      await cancelFloatDrag();
      await dockPanel(panelId);
    } catch (err) {
      console.error(`[PanelWindow] Failed to close panel "${panelId}":`, err);
      getCurrentWindow().close();
    }
  }, [panelId]);

  const handleMinimize = useCallback(async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (err) {
      console.error(`[PanelWindow] Failed to minimize panel "${panelId}":`, err);
    }
  }, [panelId]);

  /** Floating Overlay windows: drag via explicit startDragging (not -webkit-app-region). */
  const handleTitlebarMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (target.closest('button, input, select, textarea')) {
        return;
      }
      e.preventDefault();
      // Must call startDragging in the same turn as mousedown — awaiting IPC first
      // drops the OS drag. Session begin runs in parallel.
      if (canRedock) {
        void beginFloatDrag(panelId).catch((err) => {
          console.error(`[PanelWindow] beginFloatDrag failed for "${panelId}":`, err);
        });
      }
      void getCurrentWindow()
        .startDragging()
        .catch((err) => {
          console.error(`[PanelWindow] startDragging failed for "${panelId}":`, err);
        });
    },
    [panelId, canRedock]
  );

  useEffect(() => {
    return () => {
      if (canRedock) {
        void cancelFloatDrag();
      }
    };
  }, [canRedock]);

  useEffect(() => {
    if (!canRedock) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    onDockAffinity((event) => {
      if (cancelled) return;
      const { panelId: id, armed } = event.payload;
      if (id !== panelId) return;
      setAffinityArmed(armed);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [panelId, canRedock]);

  // Escape cancels affinity session when possible (may not interrupt OS drag).
  useEffect(() => {
    if (!canRedock) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        void cancelFloatDrag();
        setAffinityArmed(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [canRedock]);

  useEffect(() => {
    let cancelled = false;
    let unlistenFn: (() => void) | null = null;

    listen<PanelStateSnapshot | PanelInfo[]>('panel-state-changed', (event) => {
      if (cancelled) return;
      const payload = event.payload;
      const panels = Array.isArray(payload) ? payload : payload.panels;
      const thisPanel = panels.find((p) => p.id === panelId);
      if (thisPanel && thisPanel.docked) {
        getCurrentWindow().close();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenFn = fn;
    });

    return () => {
      cancelled = true;
      if (unlistenFn) unlistenFn();
    };
  }, [panelId]);

  useEffect(() => {
    const reportBounds = () => {
      if (bounceTimerRef.current !== null) {
        clearTimeout(bounceTimerRef.current);
      }
      bounceTimerRef.current = setTimeout(async () => {
        try {
          const win = getCurrentWindow();
          const scaleFactor = await win.scaleFactor();
          const position = await win.outerPosition();
          const size = await win.outerSize();
          const logicalSize = size.toLogical(scaleFactor);
          const logicalPos = position.toLogical(scaleFactor);
          await savePanelBounds(
            panelId,
            Math.round(logicalPos.x),
            Math.round(logicalPos.y),
            Math.round(logicalSize.width),
            Math.round(logicalSize.height)
          );
        } catch (err) {
          console.error(`[PanelWindow] Failed to save bounds for "${panelId}":`, err);
        }
      }, 500);
    };

    window.addEventListener('resize', reportBounds);

    let unlistenMove: (() => void) | null = null;
    let cancelled = false;
    const win = getCurrentWindow();
    win
      .onMoved(() => {
        if (!cancelled) reportBounds();
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlistenMove = fn;
      });

    return () => {
      cancelled = true;
      window.removeEventListener('resize', reportBounds);
      if (bounceTimerRef.current !== null) clearTimeout(bounceTimerRef.current);
      if (unlistenMove) unlistenMove();
    };
  }, [panelId]);

  const renderPanelContent = () => {
    switch (panelId) {
      case 'effect':
        return <EffectsFeature />;
      case 'layers':
        return <LayersFeature />;
      case 'colorlab':
        return <ColorLabFeature variant="full" />;
      case 'preview':
        return <FloatingPreview />;
      case 'preferences':
        return <PreferencesFeature />;
      default:
        return <div className={cn("panel-window-content-placeholder")}>Unknown Panel</div>;
    }
  };

  return (
    <div
      className={cn('panel-window', affinityArmed && 'panel-window-affinity')}
      data-panel-id={panelId}
    >
      <div
        className={cn('panel-window-titlebar', affinityArmed && 'panel-window-titlebar-affinity')}
        onMouseDown={handleTitlebarMouseDown}
      >
        <div className={cn("panel-window-titlebar-actions")}>
          <button
            className={cn("panel-window-btn", "panel-window-btn-close")}
            onClick={handleClose}
            onMouseDown={(e) => e.stopPropagation()}
            title={
              panelId === 'preferences'
                ? 'Close'
                : panelId === 'preview'
                  ? 'Return preview to main window'
                  : 'Dock panel back to sidebar'
            }
            type="button"
          >
            <img src="/icons/clouse-window-icon.svg" width="14" height="14" alt="" />
          </button>
          <button
            className={cn("panel-window-btn", "panel-window-btn-minimize")}
            onClick={handleMinimize}
            onMouseDown={(e) => e.stopPropagation()}
            title="Minimize"
            type="button"
          >
            <img src="/icons/hide-window-icon.svg" width="14" height="14" alt="" />
          </button>
        </div>
        <div className={cn("panel-window-titlebar-drag")}>
          <div className={cn("panel-window-titlebar-lines")}></div>
          <span className={cn("panel-window-title")}>
            {displayName}
          </span>
          <div className={cn("panel-window-titlebar-lines")}></div>
        </div>
      </div>
      <div className={cn("panel-window-content")} data-floating-panel-content>
        {renderPanelContent()}
      </div>
    </div>
  );
}

export default PanelWindow;
