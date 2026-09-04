import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import MenuBar from '../components/MenuBar';
import DocumentTabBar from '../features/document/DocumentTabBar';
import { WindowShell } from '../components/WindowShell';
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
import { refreshDocument, setDocumentMeta } from './slices/documentSlice';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { refreshTabs, tabsChanged } from './slices/tabsSlice';
import { useShell } from './shell/ShellContext';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import {
  onDockAffinity,
  onPanelStateChanged,
  onNativeMenu,
  onAppQuitRequested,
  onTabsChanged,
  allowAppExit,
  confirmAppQuit,
  swapSidebars as swapSidebarPanels,
  undockPanelWithSize,
  type DockAffinityEvent,
} from '../shared/ipc';
import PreviewSlot from '../features/preview/PreviewSlot';
import DockedSidebar, { sidebarEffectiveWidth } from '../features/panels/DockedSidebar';
import SidebarCollapseStrip from '../features/panels/SidebarCollapseStrip';
import FlexLayoutContainer from '../components/FlexLayoutContainer';
import ResizeHandle from '../components/common/ResizeHandle';
import { isPanelOnFlexLayout } from '../factories/layoutPanelFactory';
import {
  findTabByComponent,
  listDockedFlexComponents,
  listFlexComponents,
  useLayoutContext,
} from '../contexts/LayoutContext';
import type { DockSide, PanelId } from '../types/panels';
import styles from './AppLayout.module.css';
import menuStyles from '../features/document/MenuBar.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import resizeStyles from '../shared/ui/ResizeHandle.module.css';
import { windowChromeTitle } from '../shared/windowTitle';
import { isTooNewFileError } from '../shared/appUpdates';
import { bind } from '../shared/ui/cn';
import Icon from '../icons/iconRegistry';

const cn = bind({ ...styles, ...menuStyles, ...previewStyles, ...resizeStyles });

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
    confirmQuit,
    confirmCloseTab,
    unsavedDialog,
  } = useWelcomeScreen();
  const { panels, visibleDocked, error: panelError } = usePanels();
  const layersError = useAppSelector((s) => s.layers.error);
  const filtersError = useAppSelector((s) => s.filters.error);
  const canUndo = useAppSelector((s) => s.undo.canUndo);
  const canRedo = useAppSelector((s) => s.undo.canRedo);

  const updates = useAppUpdates({
    autoCheckOnLaunch: true,
    confirmRestart: confirmQuit,
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
    });
    void getCurrentWindow()
      .setTitle(title)
      .catch(() => {
        /* panel webviews may lack set-title; main title is enough */
      });
  }, [doc.dirty, doc.hasDocument, doc.projectPath, doc.sourcePath]);

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

  const {
    left: leftLayout,
    right: rightLayout,
    layoutEpoch,
    floatPanel,
    swapFlexSides,
    layoutToast,
    clearLayoutToast,
  } = useLayoutContext();
  // layoutEpoch: re-read docked vs floating after in-place float/dock mutations.
  void layoutEpoch;

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

  const leftPanelsAll  = visibleDocked('left');
  const rightPanelsAll = visibleDocked('right');

  // Legacy docked panels only (Preview still uses PanelManager; dockables are on FlexLayout).
  const leftPanels  = leftPanelsAll.filter( (id): id is PanelId => !isPanelOnFlexLayout(id));
  const rightPanels = rightPanelsAll.filter((id): id is PanelId => !isPanelOnFlexLayout(id));

  // Flex panel sides come from FlexLayout models (source of truth after cross-side moves).
  const leftFlexIds = listFlexComponents(leftLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const rightFlexIds = listFlexComponents(rightLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const leftDockedFlexIds = listDockedFlexComponents(leftLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const rightDockedFlexIds = listDockedFlexComponents(rightLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );

  // Any flex tab (incl. floating) keeps Layout mounted so OS popouts stay alive.
  const leftHasFlex  = leftFlexIds.length > 0;
  const rightHasFlex = rightFlexIds.length > 0;
  // Only docked flex panels reserve sidebar width — floated-away panels free the dock.
  const leftHasDockedFlex  = leftDockedFlexIds.length > 0;
  const rightHasDockedFlex = rightDockedFlexIds.length > 0;
  const leftFlexOnly  = leftHasDockedFlex  && leftPanels.length === 0;
  const rightFlexOnly = rightHasDockedFlex && rightPanels.length === 0;
  const leftMixed  = leftHasDockedFlex  && leftPanels.length > 0;
  const rightMixed = rightHasDockedFlex && rightPanels.length > 0;
  // Floated-only side: keep a zero-width Layout host + empty drop edge for redock.
  const leftFloatHostOnly  = leftHasFlex  && !leftHasDockedFlex && leftPanels.length === 0;
  const rightFloatHostOnly = rightHasFlex && !rightHasDockedFlex && rightPanels.length === 0;

  const leftW = focusMode
    ? 0
    : leftHasDockedFlex
      ? (leftSidebar.collapsed  ? 40 : leftSidebar.width)
      : sidebarEffectiveWidth(leftPanels.length,  leftSidebar.collapsed,  leftSidebar.width);
  const rightW = focusMode
    ? 0
    : rightHasDockedFlex
      ? (rightSidebar.collapsed ? 40 : rightSidebar.width)
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

  // Expand the sidebar a FlexLayout popout redocks onto.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<{ panelId: string; side: 'left' | 'right' }>(
      'flex-panel-dock-request',
      (event) => {
        if (cancelled) return;
        const { side } = event.payload;
        if (side === 'left' || side === 'right') {
          setSidebarCollapsed(side, false);
        }
      },
    ).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setSidebarCollapsed]);

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

  const resizeColumnWidth = useCallback(
    (side: DockSide, delta: number, allowCollapse: boolean) => {
      setSidebarWidth(side, (w) => {
        const signedDelta = side === 'right' ? -delta : delta;
        const newW = w + signedDelta;
        if (allowCollapse && newW < 220) {
          requestAnimationFrame(() => setSidebarCollapsed(side, true));
          return w;
        }
        return Math.min(600, Math.max(240, newW));
      });
    },
    [setSidebarWidth, setSidebarCollapsed]
  );

  const resizeColumnSplit = useCallback(
    (side: DockSide, delta: number) => {
      setSplitRatio(side, (prev) => {
        // Keep both panes usable.
        const next = prev + delta / 400;
        return Math.min(0.85, Math.max(0.15, next));
      });
    },
    [setSplitRatio]
  );

  useEffect(() => {
    for (const p of panels) {
      if (prevDockedRef.current[p.id] === undefined) {
        prevDockedRef.current[p.id] = p.docked;
        prevSideRef.current[p.id] = p.dock_side;
      }
    }
  }, [panels]);

  const handleOpenColorLab = useCallback(() => {
    const onLeft = leftLayout.model
      ? findTabByComponent(leftLayout.model, 'colorlab')
      : null;
    const onRight = rightLayout.model
      ? findTabByComponent(rightLayout.model, 'colorlab')
      : null;
    if (onLeft && !onLeft.isFloating()) {
      floatPanel('left', 'colorlab');
      return;
    }
    if (onRight && !onRight.isFloating()) {
      floatPanel('right', 'colorlab');
      return;
    }
    // Already floating (or missing) — FlexPopoutChrome / model already owns it.
  }, [floatPanel, leftLayout.model, rightLayout.model]);

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
          if (canUndo && doc.docId != null) void dispatch(undoDocument(doc.docId));
          break;
        case 'redo':
          if (canRedo && doc.docId != null) void dispatch(redoDocument(doc.docId));
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
      // Legacy PanelManager orders (Preview etc.) + shell column prefs + Flex models.
      await swapSidebarPanels();
      swapSidebars();
      swapFlexSides();
    } catch (err) {
      console.error('Swap sidebars failed:', err);
    }
  }, [swapSidebars, swapFlexSides]);

  const currentError = doc.error || layersError || filtersError;
  const toastError =
    currentError && !isTooNewFileError(currentError) ? currentError : null;
  const displayError = toastError && toastError !== dismissedError ? toastError : null;
  const displayPanelError = panelError && panelError !== dismissedPanelError ? panelError : null;

  const gridTemplateColumns = `${leftW}px 1fr ${rightW}px`;

  return (
    <WindowShell
      titlebarContent={
        <>
          <div data-tauri-drag-region="false">
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
              onUndo={() => {
                if (doc.docId != null) void dispatch(undoDocument(doc.docId));
              }}
              onRedo={() => {
                if (doc.docId != null) void dispatch(redoDocument(doc.docId));
              }}
            />
          </div>
          <div className="titlebar-filename">{windowChromeTitle({
            dirty: false,
            hasDocument: false,
            projectPath: null,
            sourcePath: null,
          })}</div>
          <div className={cn('toolbar-icon-group')} data-tauri-drag-region="false">
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
        </>
      }
    >
    <div className={cn('app-layout', 'app-layout-dual')} style={{ gridTemplateColumns }}>
      <DocumentTabBar 
        onOpenFile={welcome.onOpenImage} 
        onCloseTab={confirmCloseTab}
      />

      {!focusMode && (
        <>
          {/* Left flex column (docked and/or floating). One stable FlexLayout mount
              so OS popouts survive when the last docked tab floats away. */}
          {(leftFlexOnly || leftFloatHostOnly) && (
            <>
              {leftFlexOnly && leftSidebar.collapsed && (
                <SidebarCollapseStrip
                  side="left"
                  panelIds={leftDockedFlexIds}
                  onExpand={() => setSidebarCollapsed('left', false)}
                  hitTargetRef={leftHitRef}
                />
              )}
              {leftFlexOnly && !leftSidebar.collapsed && (
                <ResizeHandle
                  direction="horizontal"
                  onResize={(d) => resizeColumnWidth('left', d, true)}
                  className={cn(
                    'sidebar-resize-handle',
                    'sidebar-resize-left',
                    'sidebar-resize-handle-left'
                  )}
                />
              )}
              {leftFloatHostOnly && (
                <div
                  className={cn(
                    'sidebar-empty-drop-edge',
                    'sidebar-empty-drop-edge-left',
                    affinity?.armed && affinity.side === 'left' && 'sidebar-empty-drop-edge-armed'
                  )}
                  data-dock-empty-edge="left"
                  ref={(el) => {
                    leftHitRef.current = el;
                  }}
                  aria-hidden
                />
              )}
              <div
                className={
                  leftFlexOnly && !leftSidebar.collapsed
                    ? cn('app-sidebar', 'sidebar-area-left')
                    : undefined
                }
                style={
                  leftFlexOnly && !leftSidebar.collapsed
                    ? { minWidth: 0, minHeight: 0, height: '100%' }
                    : {
                        position: 'fixed',
                        width: 0,
                        height: 0,
                        overflow: 'hidden',
                        pointerEvents: 'none',
                        opacity: 0,
                      }
                }
                aria-hidden={!(leftFlexOnly && !leftSidebar.collapsed)}
              >
                <FlexLayoutContainer side="left" style={{ height: '100%', minWidth: 0 }} />
              </div>
            </>
          )}
          {/* Left mixed collapsed: one strip for flex + legacy panels on this side. */}
          {leftMixed && leftSidebar.collapsed && (
            <SidebarCollapseStrip
              side="left"
              panelIds={[...leftDockedFlexIds, ...leftPanels]}
              onExpand={() => setSidebarCollapsed('left', false)}
              hitTargetRef={leftHitRef}
            />
          )}
          {/* Left mixed expanded: vertical stack — FlexLayout above, legacy below. */}
          {leftMixed && !leftSidebar.collapsed && (
            <div
              className={cn('sidebar-area-left')}
              style={{
                display: 'flex',
                flexDirection: 'column',
                minWidth: 0,
                minHeight: 0,
                overflow: 'hidden',
                position: 'relative',
              }}
            >
              <ResizeHandle
                direction="horizontal"
                onResize={(d) => resizeColumnWidth('left', d, true)}
                className={cn('sidebar-resize-handle', 'sidebar-resize-handle-left')}
                style={{ position: 'absolute', right: 0, top: 0, bottom: 0 }}
              />
              <div style={{ flex: leftSplitRatio, minHeight: 0, overflow: 'hidden' }}>
                <FlexLayoutContainer side="left" style={{ height: '100%', minWidth: 0 }} />
              </div>
              <ResizeHandle
                direction="vertical"
                onResize={(d) => resizeColumnSplit('left', d)}
              />
              <div style={{ flex: 1 - leftSplitRatio, minHeight: 0, overflow: 'hidden' }}>
                <DockedSidebar
                  side="left"
                  panelIds={leftPanels}
                  width={leftSidebar.width}
                  collapsed={false}
                  splitRatio={leftSplitRatio}
                  affinity={affinity}
                  oppositeHitRef={rightHitRef}
                  hitTargetRef={leftHitRef}
                  onCollapsedChange={(c) => setSidebarCollapsed('left', c)}
                  onWidthChange={(w) => setSidebarWidth('left', w)}
                  onSplitRatioChange={(r) => setSplitRatio('left', r)}
                  embedded
                />
              </div>
            </div>
          )}
          {/* Left legacy only (no docked flex on left). */}
          {!leftHasDockedFlex && leftPanels.length > 0 && (
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
          {/* Floated flex + legacy: keep Layout alive off-screen. */}
          {!leftHasDockedFlex && leftHasFlex && leftPanels.length > 0 && (
            <div
              aria-hidden
              style={{
                position: 'fixed',
                width: 0,
                height: 0,
                overflow: 'hidden',
                pointerEvents: 'none',
                opacity: 0,
              }}
            >
              <FlexLayoutContainer side="left" />
            </div>
          )}
        </>
      )}

      <div
        className={cn('app-canvas')}
        data-panel-id="preview"
        style={previewBackgroundStyle(previewBackground)}
      >
        <PreviewSlot onTitleBarMouseDown={handlePreviewTitleMouseDown} welcome={welcome} />
      </div>

      {!focusMode && (
        <>
          {/* Right flex column — stable FlexLayout mount (see left). */}
          {(rightFlexOnly || rightFloatHostOnly) && (
            <>
              {rightFlexOnly && rightSidebar.collapsed && (
                <SidebarCollapseStrip
                  side="right"
                  panelIds={rightDockedFlexIds}
                  onExpand={() => setSidebarCollapsed('right', false)}
                  hitTargetRef={rightHitRef}
                />
              )}
              {rightFlexOnly && !rightSidebar.collapsed && (
                <ResizeHandle
                  direction="horizontal"
                  onResize={(d) => resizeColumnWidth('right', d, true)}
                  className={cn('sidebar-resize-handle', 'sidebar-resize-right')}
                />
              )}
              {rightFloatHostOnly && (
                <div
                  className={cn(
                    'sidebar-empty-drop-edge',
                    'sidebar-empty-drop-edge-right',
                    affinity?.armed && affinity.side === 'right' && 'sidebar-empty-drop-edge-armed'
                  )}
                  data-dock-empty-edge="right"
                  ref={(el) => {
                    rightHitRef.current = el;
                  }}
                  aria-hidden
                />
              )}
              <div
                className={
                  rightFlexOnly && !rightSidebar.collapsed
                    ? cn('app-sidebar', 'sidebar-area-right')
                    : undefined
                }
                style={
                  rightFlexOnly && !rightSidebar.collapsed
                    ? { minWidth: 0, minHeight: 0, height: '100%' }
                    : {
                        position: 'fixed',
                        width: 0,
                        height: 0,
                        overflow: 'hidden',
                        pointerEvents: 'none',
                        opacity: 0,
                      }
                }
                aria-hidden={!(rightFlexOnly && !rightSidebar.collapsed)}
              >
                <FlexLayoutContainer side="right" style={{ height: '100%', minWidth: 0 }} />
              </div>
            </>
          )}
          {/* Right mixed collapsed */}
          {rightMixed && rightSidebar.collapsed && (
            <SidebarCollapseStrip
              side="right"
              panelIds={[...rightDockedFlexIds, ...rightPanels]}
              onExpand={() => setSidebarCollapsed('right', false)}
              hitTargetRef={rightHitRef}
            />
          )}
          {/* Right mixed expanded: vertical stack — FlexLayout above, legacy (colorlab) below. */}
          {rightMixed && !rightSidebar.collapsed && (
            <div
              className={cn('sidebar-area-right')}
              style={{
                display: 'flex',
                flexDirection: 'column',
                minWidth: 0,
                minHeight: 0,
                overflow: 'hidden',
                position: 'relative',
              }}
            >
              <ResizeHandle
                direction="horizontal"
                onResize={(d) => resizeColumnWidth('right', d, true)}
                className={cn('sidebar-resize-handle')}
                style={{ position: 'absolute', left: 0, top: 0, bottom: 0 }}
              />
              <div style={{ flex: rightSplitRatio, minHeight: 0, overflow: 'hidden' }}>
                <FlexLayoutContainer side="right" style={{ height: '100%', minWidth: 0 }} />
              </div>
              <ResizeHandle
                direction="vertical"
                onResize={(d) => resizeColumnSplit('right', d)}
              />
              <div style={{ flex: 1 - rightSplitRatio, minHeight: 0, overflow: 'hidden' }}>
                <DockedSidebar
                  side="right"
                  panelIds={rightPanels}
                  width={rightSidebar.width}
                  collapsed={false}
                  splitRatio={rightSplitRatio}
                  affinity={affinity}
                  oppositeHitRef={leftHitRef}
                  hitTargetRef={rightHitRef}
                  onCollapsedChange={(c) => setSidebarCollapsed('right', c)}
                  onWidthChange={(w) => setSidebarWidth('right', w)}
                  onSplitRatioChange={(r) => setSplitRatio('right', r)}
                  embedded
                />
              </div>
            </div>
          )}
          {/* Right legacy only (no docked flex on right). */}
          {!rightHasDockedFlex && rightPanels.length > 0 && (
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
          {!rightHasDockedFlex && rightHasFlex && rightPanels.length > 0 && (
            <div
              aria-hidden
              style={{
                position: 'fixed',
                width: 0,
                height: 0,
                overflow: 'hidden',
                pointerEvents: 'none',
                opacity: 0,
              }}
            >
              <FlexLayoutContainer side="right" />
            </div>
          )}
        </>
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
        message={layoutToast}
        type="success"
        onDismiss={clearLayoutToast}
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
    </WindowShell>
  );
}
