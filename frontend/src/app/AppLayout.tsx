import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import MenuBar from '../components/MenuBar';
import Notification from '../components/common/Notification';
import NewProjectDialog from '../components/NewProjectDialog';
import HelpDialog from '../components/HelpDialog';
import PreferencesDialog from '../features/preferences/PreferencesDialog';
import { useWelcomeScreen } from '../hooks/useWelcomeScreen';
import { usePanels } from '../hooks/usePanels';
import { registerDocumentCommands, registerLayoutCommands } from '../features/shortcuts/commandRegistry';
import { useAppUpdates } from '../hooks/useAppUpdates';
import { useAppDispatch, useAppSelector } from './hooks';
import { refreshFilters } from './slices/filtersSlice';
import { refreshLayers } from './slices/layersSlice';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { setDocumentMeta } from './slices/documentSlice';
import { useShell } from './shell/ShellContext';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import {
  onDockAffinity,
  onPanelStateChanged,
  onNativeMenu,
  onAppQuitRequested,
  allowAppExit,
  confirmAppQuit,
  swapSidebars as swapSidebarPanels,
  undockPanel,
  undockPanelWithSize,
  type DockAffinityEvent,
} from '../shared/ipc';
import PreviewSlot from '../features/preview/PreviewSlot';
import DockedSidebar, { sidebarEffectiveWidth } from '../features/panels/DockedSidebar';
import styles from './AppLayout.module.css';
import menuStyles from '../features/document/MenuBar.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import { windowChromeTitle } from '../shared/windowTitle';
import { isTooNewFileError } from '../shared/appUpdates';
import { bind } from '../shared/ui/cn';
import Icon from '../icons/iconRegistry';

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

  const updates = useAppUpdates({
    autoCheckOnLaunch: true,
    confirmRestart: confirmReplace,
    fileError: doc.error,
    onStatus: (message, kind) => {
      dispatch(
        setDocumentMeta(
          kind === 'error'
            ? { error: message }
            : { notification: message, error: null }
        )
      );
    },
    clearFileError: () => dispatch(setDocumentMeta({ error: null })),
  });

  const allowCloseRef = useRef(false);
  const quitInFlightRef = useRef(false);
  const confirmReplaceRef = useRef(confirmReplace);
  confirmReplaceRef.current = confirmReplace;

  const requestQuit = useCallback(async () => {
    if (quitInFlightRef.current) return;
    quitInFlightRef.current = true;
    try {
      const ok = await confirmReplaceRef.current();
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
    });
    void getCurrentWindow()
      .setTitle(title)
      .catch(() => {
        /* panel webviews may lack set-title; main title is enough */
      });
  }, [doc.dirty, doc.hasDocument, doc.projectPath, doc.sourcePath]);

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

  const {
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    setSidebarCollapsed,
    setSidebarWidth,
    setSplitRatio,
    swapSidebars,
    previewBackground,
  } = useShell();

  const [dismissedError, setDismissedError] = useState<string | null>(null);
  const [dismissedPanelError, setDismissedPanelError] = useState<string | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const [affinity, setAffinity] = useState<DockAffinityEvent | null>(null);
  const prevDockedRef = useRef<Record<string, boolean>>({});
  const prevSideRef = useRef<Record<string, string | null | undefined>>({});
  const leftHitRef = useRef<HTMLElement | null>(null);
  const rightHitRef = useRef<HTMLElement | null>(null);

  const leftPanels = visibleDocked('left');
  const rightPanels = visibleDocked('right');

  const leftW = focusMode
    ? 0
    : sidebarEffectiveWidth(leftPanels.length, leftSidebar.collapsed, leftSidebar.width);
  const rightW = focusMode
    ? 0
    : sidebarEffectiveWidth(rightPanels.length, rightSidebar.collapsed, rightSidebar.width);

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

  const handleOpenPreferences = useCallback(() => {
    setPreferencesOpen(true);
  }, []);

  const handleOpenHelp = useCallback(() => {
    setHelpOpen(true);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void onNativeMenu((id) => {
      switch (id) {
        case 'new-project':
          welcome.onNewProject();
          break;
        case 'open-image':
          welcome.onOpenImage();
          break;
        case 'import-image-layer':
          if (doc.hasDocument) void doc.importImageLayer();
          break;
        case 'open-project':
          welcome.onOpenProject();
          break;
        case 'save-project':
          if (doc.hasDocument) onSaveProject();
          break;
        case 'save-project-as':
          if (doc.hasDocument) onSaveProjectAs();
          break;
        case 'save-export':
          if (doc.hasDocument) onSaveImage();
          break;
        case 'undo':
          if (canUndo) void dispatch(undoDocument());
          break;
        case 'redo':
          if (canRedo) void dispatch(redoDocument());
          break;
        case 'export-pattern':
          if (doc.hasDocument) void doc.exportPattern();
          break;
        case 'import-pattern':
          if (doc.hasDocument) void doc.importPattern();
          break;
        case 'color-lab':
          void handleOpenColorLab();
          break;
        case 'preferences':
          handleOpenPreferences();
          break;
        case 'about':
        case 'help':
          handleOpenHelp();
          break;
        case 'help-check-updates':
        case 'check-updates':
          void updates.checkForUpdates();
          break;
        case 'quit':
          void requestQuit();
          break;
        default:
          break;
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [
    canRedo,
    canUndo,
    dispatch,
    doc,
    handleOpenColorLab,
    handleOpenHelp,
    handleOpenPreferences,
    onSaveImage,
    onSaveProject,
    onSaveProjectAs,
    requestQuit,
    updates,
    welcome,
  ]);

  useEffect(() => {
    return registerDocumentCommands({
      newProject: welcome.onNewProject,
      openImage: welcome.onOpenImage,
      openProject: welcome.onOpenProject,
      saveProject: onSaveProject,
      saveProjectAs: onSaveProjectAs,
      openPreferences: handleOpenPreferences,
    });
  }, [
    welcome.onNewProject,
    welcome.onOpenImage,
    welcome.onOpenProject,
    onSaveProject,
    onSaveProjectAs,
    handleOpenPreferences,
  ]);

  useEffect(() => {
    return registerLayoutCommands({
      toggleFocusMode: () => setFocusMode((on) => !on),
    });
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

  const handleSwapSidebars = useCallback(async () => {
    try {
      await swapSidebarPanels();
      swapSidebars();
    } catch (err) {
      console.error('Swap sidebars failed:', err);
    }
  }, [swapSidebars]);

  const currentError = doc.error || layersError || filtersError;
  const toastError =
    currentError && !isTooNewFileError(currentError) ? currentError : null;
  const displayError = toastError && toastError !== dismissedError ? toastError : null;
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
          onImportImageLayer={() => void doc.importImageLayer()}
          onSaveImage={onSaveImage}
          onOpenProject={welcome.onOpenProject}
          onOpenRecent={welcome.onOpenRecent}
          onSaveProject={onSaveProject}
          onSaveProjectAs={onSaveProjectAs}
          onExportPattern={() => void doc.exportPattern()}
          onImportPattern={() => void doc.importPattern()}
          onOpenColorLab={handleOpenColorLab}
          onOpenPreferences={handleOpenPreferences}
          onOpenHelp={handleOpenHelp}
          onUndo={() => void dispatch(undoDocument())}
          onRedo={() => void dispatch(redoDocument())}
        />
        <div className={cn('toolbar-spacer')} />
        <div className={cn('toolbar-icon-group')}>
          <button
            type="button"
            className={cn('sidebar-changer-icon-btn')}
            onClick={() => void handleSwapSidebars()}
            title="Swap left and right sidebars"
            aria-label="Swap left and right sidebars"
          >
            <Icon name="sidebar-swap" width={16} height={16} />
          </button>
          <button
            type="button"
            className={cn(
              'sidebar-changer-icon-btn',
              focusMode && 'sidebar-changer-icon-btn-active'
            )}
            onClick={() => setFocusMode((on) => !on)}
            title={focusMode ? 'Exit focus mode' : 'Focus mode — hide sidebars'}
            aria-label={focusMode ? 'Exit focus mode' : 'Focus mode'}
            aria-pressed={focusMode}
          >
            <Icon name="focus-mode" width={16} height={16} />
          </button>
        </div>
      </div>

      {!focusMode && (
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
      )}

      <div
        className={cn('app-canvas')}
        data-panel-id="preview"
        style={previewBackgroundStyle(previewBackground)}
      >
        <PreviewSlot onTitleBarMouseDown={handlePreviewTitleMouseDown} welcome={welcome} />
      </div>

      {!focusMode && (
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
      )}

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
      <PreferencesDialog isOpen={preferencesOpen} onClose={() => setPreferencesOpen(false)} />
      <HelpDialog
        isOpen={helpOpen}
        version={updates.version}
        checking={updates.checking}
        onClose={() => setHelpOpen(false)}
        onCheckForUpdates={() => void updates.checkForUpdates()}
      />
      {unsavedDialog}
      {updates.dialogs}
      {doc.svgDialog}
    </div>
  );
}
