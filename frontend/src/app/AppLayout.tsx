import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import MenuBar from '../components/MenuBar';
import Notification from '../components/common/Notification';
import NewProjectDialog from '../components/NewProjectDialog';
import { useWelcomeScreen } from '../hooks/useWelcomeScreen';
import { usePanels } from '../hooks/usePanels';
import { useUndoShortcuts } from '../hooks/useUndoShortcuts';
import { useAppDispatch, useAppSelector } from './hooks';
import { refreshFilters } from './slices/filtersSlice';
import { refreshLayers } from './slices/layersSlice';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { useShell } from './shell/ShellContext';
import {
  onDockAffinity,
  onPanelStateChanged,
  moveAllPanelsToSide,
  undockPanel,
  undockPanelWithSize,
  type DockAffinityEvent,
} from '../shared/ipc';
import type { DockSide } from '../types/panels';
import PreviewSlot from '../features/preview/PreviewSlot';
import DockedSidebar, { sidebarEffectiveWidth } from '../features/panels/DockedSidebar';
import styles from './AppLayout.module.css';
import menuStyles from '../features/document/MenuBar.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import { projectBasename } from '../shared/unsavedGuard';
import { bind } from '../shared/ui/cn';

const cn = bind({ ...styles, ...menuStyles, ...previewStyles });

const PREVIEW_UNDOCK_THRESHOLD_PX = 5;

/**
 * Main shell layout: menubar + optional left sidebar | canvas | optional right sidebar.
 */
export default function AppLayout() {
  const dispatch = useAppDispatch();
  const {
    doc,
    welcome,
    newProjectOpen,
    closeNewProject,
    handleCreate,
    onSaveImage,
    onSaveProject,
    onSaveProjectAs,
    confirmReplace,
    unsavedDialog,
  } = useWelcomeScreen();
  const { panels, visibleDocked, error: panelError } = usePanels();
  const layersError = useAppSelector((s) => s.layers.error);
  const filtersError = useAppSelector((s) => s.filters.error);
  const canUndo = useAppSelector((s) => s.undo.canUndo);
  const canRedo = useAppSelector((s) => s.undo.canRedo);
  useUndoShortcuts();

  const allowCloseRef = useRef(false);
  const confirmReplaceRef = useRef(confirmReplace);
  confirmReplaceRef.current = confirmReplace;

  useEffect(() => {
    const bullet = doc.hasDocument && doc.dirty ? '• ' : '';
    const name = doc.hasDocument ? projectBasename(doc.projectPath) : 'Untitled';
    void getCurrentWindow().setTitle(`${bullet}${name} — Dither Engine`);
  }, [doc.dirty, doc.hasDocument, doc.projectPath]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    const win = getCurrentWindow();
    void win
      .onCloseRequested(async (event) => {
        if (allowCloseRef.current) return;
        event.preventDefault();
        const ok = await confirmReplaceRef.current();
        if (!ok) return;
        allowCloseRef.current = true;
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
  const {
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    setSidebarCollapsed,
    setSidebarWidth,
    setSplitRatio,
  } = useShell();

  const [dismissedError, setDismissedError] = useState<string | null>(null);
  const [dismissedPanelError, setDismissedPanelError] = useState<string | null>(null);
  const [affinity, setAffinity] = useState<DockAffinityEvent | null>(null);
  const prevDockedRef = useRef<Record<string, boolean>>({});
  const prevSideRef = useRef<Record<string, string | null | undefined>>({});
  const leftHitRef = useRef<HTMLElement | null>(null);
  const rightHitRef = useRef<HTMLElement | null>(null);

  const leftPanels = visibleDocked('left');
  const rightPanels = visibleDocked('right');

  const leftW = sidebarEffectiveWidth(
    leftPanels.length,
    leftSidebar.collapsed,
    leftSidebar.width
  );
  const rightW = sidebarEffectiveWidth(
    rightPanels.length,
    rightSidebar.collapsed,
    rightSidebar.width
  );

  useEffect(() => {
    void dispatch(refreshLayers(doc.docId));
    if (doc.docId !== null) {
      void dispatch(refreshFilters());
    }
  }, [dispatch, doc.docId]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    onDockAffinity((event) => {
      if (cancelled) return;
      const payload = event.payload;
      if (!payload.armed) {
        setAffinity(null);
        return;
      }
      setAffinity(payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Auto-expand the side a panel redocks onto.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    onPanelStateChanged((event) => {
      if (cancelled) return;
      const raw = event.payload;
      const list = Array.isArray(raw) ? raw : raw.panels;
      for (const p of list) {
        const wasDocked = prevDockedRef.current[p.id];
        const prevSide = prevSideRef.current[p.id];
        if (wasDocked === false && p.docked && p.dock_side) {
          setAffinity(null);
          setSidebarCollapsed(p.dock_side, false);
        } else if (p.docked && p.dock_side && prevSide && prevSide !== p.dock_side) {
          setSidebarCollapsed(p.dock_side, false);
        }
        prevDockedRef.current[p.id] = p.docked;
        prevSideRef.current[p.id] = p.dock_side;
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setSidebarCollapsed]);

  useEffect(() => {
    for (const p of panels) {
      if (prevDockedRef.current[p.id] === undefined) {
        prevDockedRef.current[p.id] = p.docked;
        prevSideRef.current[p.id] = p.dock_side;
      }
    }
  }, [panels]);

  const handleOpenColorLab = useCallback(async () => {
    try {
      await undockPanel('colorlab');
    } catch (err) {
      console.error('Open Color Lab failed:', err);
    }
  }, []);

  const handleOpenPreferences = useCallback(async () => {
    try {
      await undockPanel('preferences');
    } catch (err) {
      console.error('Open Preferences failed:', err);
    }
  }, []);

  /** Drag Preview titlebar past threshold → floating window (preview is floating-only). */
  const handlePreviewTitleMouseDown = useCallback((e: ReactMouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();

    const startX = e.clientX;
    const startY = e.clientY;
    let dragged = false;

    const onMove = (ev: MouseEvent) => {
      if (dragged) return;
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (Math.hypot(dx, dy) >= PREVIEW_UNDOCK_THRESHOLD_PX) {
        dragged = true;
        document.body.style.userSelect = 'none';
      }
    };

    const onUp = (ev: MouseEvent) => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.userSelect = '';
      if (!dragged) return;

      const el = document.querySelector<HTMLElement>('[data-panel-id="preview"]');
      if (!el) return;
      const rect = el.getBoundingClientRect();
      void undockPanelWithSize(
        'preview',
        Math.round(rect.width),
        Math.round(rect.height),
        Math.round(ev.screenX),
        Math.round(ev.screenY)
      ).catch((err) => console.error('Preview undock failed:', err));
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }, []);

  const handleMoveAllToSide = useCallback(async (side: DockSide) => {
    try {
      await moveAllPanelsToSide(side);
    } catch (err) {
      console.error(`Move all panels to ${side} failed:`, err);
    }
  }, []);

  const currentError = doc.error || layersError || filtersError;
  const displayError = currentError && currentError !== dismissedError ? currentError : null;
  const displayPanelError = panelError && panelError !== dismissedPanelError ? panelError : null;

  const gridTemplateColumns = `${leftW}px 1fr ${rightW}px`;

  return (
    <div className={cn('app-layout', 'app-layout-dual')} style={{ gridTemplateColumns }}>
      <div className={cn('app-toolbar')}>
        <MenuBar
          hasDocument={doc.hasDocument}
          canUndo={canUndo}
          canRedo={canRedo}
          recentEntries={welcome.recentEntries}
          onNewProject={welcome.onNewProject}
          onOpenImage={welcome.onOpenImage}
          onSaveImage={onSaveImage}
          onOpenProject={welcome.onOpenProject}
          onOpenRecent={welcome.onOpenRecent}
          onSaveProject={onSaveProject}
          onSaveProjectAs={onSaveProjectAs}
          onExportPattern={() => void doc.exportPattern()}
          onImportPattern={() => void doc.importPattern()}
          onOpenColorLab={handleOpenColorLab}
          onOpenPreferences={handleOpenPreferences}
          onUndo={() => void dispatch(undoDocument())}
          onRedo={() => void dispatch(redoDocument())}
        />
        <div className={cn('toolbar-spacer')} />
        <button
          type="button"
          className={cn('sidebar-changer-icon-btn')}
          onClick={() => void handleMoveAllToSide('left')}
          title="Move all panels to left"
          aria-label="Move all panels to left"
        >
          <span className={cn('icon')}></span>
        </button>
        <button
          type="button"
          className={cn('sidebar-changer-icon-btn')}
          onClick={() => void handleMoveAllToSide('right')}
          title="Move all panels to right"
          aria-label="Move all panels to right"
        >
          <span className={cn('icon', 'icon-move-right')}></span>
        </button>
      </div>

      <DockedSidebar
        side="left"
        panelIds={leftPanels}
        width={leftSidebar.width}
        collapsed={leftSidebar.collapsed}
        splitRatio={leftSplitRatio}
        affinity={affinity}
        oppositeHitRef={rightHitRef}
        hitTargetRef={leftHitRef}
        onCollapsedChange={(c) => setSidebarCollapsed('left', c)}
        onWidthChange={(w) => setSidebarWidth('left', w)}
        onSplitRatioChange={(r) => setSplitRatio('left', r)}
      />

      <div className={cn('app-canvas')} data-panel-id="preview">
        <PreviewSlot onTitleBarMouseDown={handlePreviewTitleMouseDown} welcome={welcome} />
      </div>

      <DockedSidebar
        side="right"
        panelIds={rightPanels}
        width={rightSidebar.width}
        collapsed={rightSidebar.collapsed}
        splitRatio={rightSplitRatio}
        affinity={affinity}
        oppositeHitRef={leftHitRef}
        hitTargetRef={rightHitRef}
        onCollapsedChange={(c) => setSidebarCollapsed('right', c)}
        onWidthChange={(w) => setSidebarWidth('right', w)}
        onSplitRatioChange={(r) => setSplitRatio('right', r)}
      />

      <Notification
        message={displayError}
        type="error"
        onDismiss={() => setDismissedError(currentError)}
      />
      <Notification
        message={doc.notification}
        type="success"
        onDismiss={doc.clearNotification}
      />
      <Notification
        message={displayPanelError}
        type="error"
        onDismiss={() => setDismissedPanelError(panelError)}
      />
      <NewProjectDialog
        isOpen={newProjectOpen}
        onClose={closeNewProject}
        onCreate={handleCreate}
      />
      {unsavedDialog}
      {doc.svgDialog}
    </div>
  );
}
