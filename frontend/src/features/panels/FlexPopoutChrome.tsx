/**
 * FlexLayout OS popout shell — Figma `heder-of-window` floating variant
 * (close / minimize / zoom + ridges + title) via shared WindowTitlebar.
 *
 * W1/B4c: titlebar drag is pointer-capture + JS setPosition on the flex-popout
 * WebviewWindow (portal from main — do not use getCurrentWindow / main
 * `window` for drag). Affinity via WindowEvent::Moved.
 *
 * Shortcuts: keydown is relayed from the popout browsing context to the main
 * shortcut engine (portaled React still runs in main JS).
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import {
  cancelFloatDrag,
  onDockAffinity,
} from '../../shared/ipc';
import { relayShortcutEvent } from '../shortcuts/shortcutRelay';
import styles from './PanelWindow.module.css';
import { bind } from '../../shared/ui/cn';
import WindowTitlebar from '../../shared/ui/WindowTitlebar';
import { useFlexPopoutJsDrag } from './useFlexPopoutJsDrag';
import { closeFlexPopoutShell, findFlexPopoutWebviewForDomWindow } from './closeFlexPopoutShell';
import {
  forgetFlexPopoutLabel,
  rememberFlexPopoutLabel,
} from './flexPopoutRegistry';
import { registerFlexPopoutDocument } from './flexPopoutStyleSync';
import { useOwnerWindow } from './useOwnerDocument';

const cn = bind(styles);

export default function FlexPopoutChrome({
  title,
  panelId,
  onDockBack,
  closeLabel = 'Dock panel back to sidebar',
  children,
}: {
  title: string;
  panelId: string;
  onDockBack: () => void;
  /** Tooltip / aria for the close control (Preview returns to canvas). */
  closeLabel?: string;
  children: React.ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const shellRef = useRef<WebviewWindow | null>(null);
  const [affinityArmed, setAffinityArmed] = useState(false);
  const { onTitlebarPointerDown, cancelActiveDrag } = useFlexPopoutJsDrag(panelId);
  const ownerWindow = useOwnerWindow(rootRef);

  // W5: mirror main <head> / theme into this popout document.
  useEffect(() => {
    const doc = rootRef.current?.ownerDocument;
    if (!doc || doc === document) return;
    return registerFlexPopoutDocument(doc);
  }, [ownerWindow]);

  // Resolve + remember this chrome's flex-popout label for close/minimize/drag.
  useEffect(() => {
    let cancelled = false;
    const view = ownerWindow;
    if (!view || view === window) return;
    const bind = () => {
      void findFlexPopoutWebviewForDomWindow(view, {
        panelId,
        allowSoleCandidate: true,
      }).then((w) => {
        if (cancelled || !w) return;
        shellRef.current = w;
        rememberFlexPopoutLabel(panelId, w.label, view);
      });
    };
    bind();
    // Tauri may create the shell a tick after the portal mounts — retry once.
    const retry = window.setTimeout(bind, 50);
    return () => {
      cancelled = true;
      window.clearTimeout(retry);
    };
  }, [panelId, ownerWindow]);

  const handleClose = useCallback(() => {
    cancelActiveDrag();
    const preferredLabel = shellRef.current?.label ?? null;
    void cancelFloatDrag(preferredLabel);
    setAffinityArmed(false);
    // Capture this popout's browsing context before unfloat unmounts the portal.
    const view = ownerWindow !== window ? ownerWindow : null;
    onDockBack();
    forgetFlexPopoutLabel(panelId);
    // dockPanel also closes via tab.getWindow(); this covers the chrome path
    // when the user clicks close on this shell (siblings stay open).
    void closeFlexPopoutShell(view, { preferredLabel, panelId });
  }, [onDockBack, cancelActiveDrag, panelId, ownerWindow]);

  const handleTitlebarPointerDown = useCallback(
    (e: React.PointerEvent) => {
      // Double-click titlebar = dock back (discoverable return for Preview).
      if (e.detail === 2) {
        e.preventDefault();
        e.stopPropagation();
        handleClose();
        return;
      }
      onTitlebarPointerDown(e);
    },
    [handleClose, onTitlebarPointerDown],
  );

  const handleMinimize = useCallback(() => {
    const w = shellRef.current;
    if (!w) return;
    void w
      .minimize()
      .catch((err) => console.error('[FlexPopoutChrome] minimize failed:', err));
  }, []);

  const handleMaximize = useCallback(() => {
    const w = shellRef.current;
    if (!w) return;
    void (async () => {
      if (await w.isMaximized()) await w.unmaximize();
      else await w.maximize();
    })().catch((err) =>
      console.error('[FlexPopoutChrome] maximize failed:', err),
    );
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

  // Relay popout keydowns → main shortcut engine; Escape cancels drag.
  // W5: listen on the popout owner window, never assume main `window`.
  useEffect(() => {
    const view = ownerWindow;
    if (!view) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        cancelActiveDrag();
        setAffinityArmed(false);
      }
      // Always relay — main window never sees popout key events otherwise.
      if (view !== window) {
        relayShortcutEvent(e);
      }
    };

    view.addEventListener('keydown', onKeyDown, true);
    return () => view.removeEventListener('keydown', onKeyDown, true);
  }, [cancelActiveDrag, ownerWindow]);

  return (
    <div
      ref={rootRef}
      className={cn('panel-window', affinityArmed && 'panel-window-affinity')}
      data-flex-popout-chrome
      style={{ height: '100%', width: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
    >
      <WindowTitlebar
        variant="floating"
        title={title}
        className={affinityArmed ? cn('panel-window-titlebar-affinity') : undefined}
        onPointerDown={handleTitlebarPointerDown}
        onClose={handleClose}
        onMinimize={handleMinimize}
        onMaximize={handleMaximize}
        closeLabel={closeLabel}
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
