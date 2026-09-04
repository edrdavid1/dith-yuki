/**
 * FlexLayout OS popout shell — Figma `heder-of-window` floating variant
 * (close / minimize / zoom + ridges + title) via shared WindowTitlebar.
 *
 * B4c: titlebar drag is JS setPosition on the flex-popout WebviewWindow +
 * mouseup on the *popout* document window (portal from main — do not use
 * getCurrentWindow / main `window` for drag). Affinity via WindowEvent::Moved.
 */

import { useCallback, useEffect, useState } from 'react';
import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import {
  cancelFloatDrag,
  onDockAffinity,
} from '../../shared/ipc';
import styles from './PanelWindow.module.css';
import { bind } from '../../shared/ui/cn';
import WindowTitlebar from '../../shared/ui/WindowTitlebar';
import { useFlexPopoutJsDrag } from './useFlexPopoutJsDrag';

const cn = bind(styles);

async function focusedFlexPopout(): Promise<WebviewWindow | null> {
  const wins = await getAllWebviewWindows();
  const flex = wins.filter((w) => w.label.startsWith('flex-popout-'));
  for (const w of flex) {
    try {
      if (await w.isFocused()) return w;
    } catch {
      /* ignore */
    }
  }
  return flex[flex.length - 1] ?? null;
}

/** Close every flex-popout OS window (dock-back cleanup). */
async function closeAllFlexPopouts(): Promise<void> {
  const wins = await getAllWebviewWindows();
  await Promise.all(
    wins
      .filter((w) => w.label.startsWith('flex-popout-'))
      .map(async (w) => {
        try {
          await w.close();
        } catch (err) {
          console.error('[FlexPopoutChrome] close failed:', err);
        }
      }),
  );
}

export default function FlexPopoutChrome({
  title,
  panelId,
  onDockBack,
  children,
}: {
  title: string;
  panelId: string;
  onDockBack: () => void;
  children: React.ReactNode;
}) {
  const [affinityArmed, setAffinityArmed] = useState(false);
  const { onTitlebarMouseDown, cancelActiveDrag } = useFlexPopoutJsDrag(panelId);

  const handleClose = useCallback(() => {
    cancelActiveDrag();
    void cancelFloatDrag();
    onDockBack();
    void closeAllFlexPopouts();
  }, [onDockBack, cancelActiveDrag]);

  const handleMinimize = useCallback(() => {
    void focusedFlexPopout()
      .then((w) => w?.minimize())
      .catch((err) => console.error('[FlexPopoutChrome] minimize failed:', err));
  }, []);

  const handleMaximize = useCallback(() => {
    void focusedFlexPopout()
      .then(async (w) => {
        if (!w) return;
        if (await w.isMaximized()) await w.unmaximize();
        else await w.maximize();
      })
      .catch((err) => console.error('[FlexPopoutChrome] maximize failed:', err));
  }, []);

  useEffect(() => {
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
  }, [panelId]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        cancelActiveDrag();
        setAffinityArmed(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [cancelActiveDrag]);

  return (
    <div
      className={cn('panel-window', affinityArmed && 'panel-window-affinity')}
      data-flex-popout-chrome
      style={{ height: '100%', width: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
    >
      <WindowTitlebar
        variant="floating"
        title={title}
        className={affinityArmed ? cn('panel-window-titlebar-affinity') : undefined}
        onMouseDown={onTitlebarMouseDown}
        onClose={handleClose}
        onMinimize={handleMinimize}
        onMaximize={handleMaximize}
        closeLabel="Dock panel back to sidebar"
      />
      <div
        className={cn('panel-window-content')}
        data-floating-panel-content
        style={{ flex: 1, minHeight: 0, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}
      >
        {children}
      </div>
    </div>
  );
}
