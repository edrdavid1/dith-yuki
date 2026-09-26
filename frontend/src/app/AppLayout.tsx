import { useCallback, useEffect, useRef, useState } from 'react';
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
import { registerDocumentCommands, registerLayoutCommands } from '../features/shortcuts/commandRegistry';
import { useAppUpdates } from '../hooks/useAppUpdates';
import { useAppDispatch, useAppSelector } from './hooks';
import { refreshFilters } from './slices/filtersSlice';
import { addLayerWithPreset, refreshLayers } from './slices/layersSlice';
import { refreshDocument, setDocumentMeta } from './slices/documentSlice';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { setSelection } from './slices/selectionSlice';
import { refreshTabs, tabsChanged } from './slices/tabsSlice';
import { useShell } from './shell/ShellContext';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import {
  onDockAffinity,
  onNativeMenu,
  onAppQuitRequested,
  onTabsChanged,
  allowAppExit,
  confirmAppQuit,
  type DockAffinityEvent,
} from '../shared/ipc';
import SidebarCollapseStrip from '../features/panels/SidebarCollapseStrip';
import { sidebarColumnWidth } from '../features/panels/panelDragMode';
import FlexLayoutContainer from '../components/FlexLayoutContainer';
import ResizeHandle from '../components/common/ResizeHandle';
import { isPanelOnFlexLayout } from '../factories/layoutPanelFactory';
import {
  isPreviewFloating,
  listDockedFlexComponents,
  listFlexComponents,
  useLayoutContext,
} from '../contexts/LayoutContext';
import type { DockSide, PanelId } from '../types/panels';
import { appEdgesAttr } from '../shared/ui/appEdges';
import { isMacOS } from '../lib/platform';
import styles from './AppLayout.module.css';
import menuStyles from '../features/document/MenuBar.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import previewWindowStyles from '../features/preview/PreviewWindow.module.css';
import resizeStyles from '../shared/ui/ResizeHandle.module.css';
import { windowChromeTitle } from '../shared/windowTitle';
import { isTooNewFileError } from '../shared/appUpdates';
import { bind } from '../shared/ui/cn';
import Icon from '../icons/iconRegistry';

const cn = bind({ ...styles, ...menuStyles, ...previewStyles, ...previewWindowStyles, ...resizeStyles });

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
    onShareProjectCopy,
    confirmQuit,
    confirmCloseTab,
    unsavedDialog,
  } = useWelcomeScreen();
  const layersError = useAppSelector((s) => s.layers.error);
  const filtersError = useAppSelector((s) => s.filters.error);
  const canUndo = useAppSelector((s) => s.undo.canUndo);
  const canRedo = useAppSelector((s) => s.undo.canRedo);
  const layerTree = useAppSelector((s) => s.layers.tree);
  const docId = useAppSelector((s) => s.document.docId);

  const applyCrossStitchPreset = useCallback(() => {
    void dispatch(addLayerWithPreset({ docId, layers: layerTree, presetId: 'cross_stitch_pattern' })).then(
      (result) => {
        if (addLayerWithPreset.fulfilled.match(result) && result.payload != null) {
          void dispatch(
            setSelection({
              layerId: result.payload.layerId,
              filterId: result.payload.filterId,
            })
          );
        }
      }
    );
  }, [dispatch, docId, layerTree]);

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
    setSidebarCollapsed,
    setSidebarWidth,
    swapSidebars,
    previewBackground,
  } = useShell();

  const {
    left: leftLayout,
    right: rightLayout,
    center: centerLayout,
    layoutEpoch,
    swapFlexSides,
    layoutToast,
    clearLayoutToast,
  } = useLayoutContext();
  // layoutEpoch: re-read docked vs floating after in-place float/dock mutations.
  void layoutEpoch;

  const [dismissedError, setDismissedError] = useState<string | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const [affinity, setAffinity] = useState<DockAffinityEvent | null>(null);
  const leftHitRef = useRef<HTMLElement | null>(null);
  const rightHitRef = useRef<HTMLElement | null>(null);

  // Flex panel sides come from FlexLayout models (source of truth).
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

  const previewFloating = isPreviewFloating(centerLayout.model);

  // Any flex tab (incl. floating) keeps Layout mounted so OS popouts stay alive.
  const leftHasFlex  = leftFlexIds.length > 0;
  const rightHasFlex = rightFlexIds.length > 0;
  // Only docked flex panels reserve sidebar width — floated-away panels free the dock.
  const leftHasDockedFlex  = leftDockedFlexIds.length > 0;
  const rightHasDockedFlex = rightDockedFlexIds.length > 0;
  const leftFlexOnly  = leftHasDockedFlex;
  const rightFlexOnly = rightHasDockedFlex;
  // Floated-only side: keep a zero-width Layout host + empty drop edge for redock.
  const leftFloatHostOnly  = leftHasFlex  && !leftHasDockedFlex;
  const rightFloatHostOnly = rightHasFlex && !rightHasDockedFlex;

  const leftW = focusMode
    ? 0
    : sidebarColumnWidth(leftHasDockedFlex, leftSidebar.collapsed, leftSidebar.width);
  const rightW = focusMode
    ? 0
    : sidebarColumnWidth(rightHasDockedFlex, rightSidebar.collapsed, rightSidebar.width);

  // macOS only — Windows OS chrome differs by version; we don't version-gate.
  const leftAppEdges = isMacOS() ? appEdgesAttr('bottom', 'left') : undefined;
  const rightAppEdges = isMacOS() ? appEdgesAttr('bottom', 'right') : undefined;
  const previewAppEdges = isMacOS()
    ? appEdgesAttr('bottom', leftW === 0 && 'left', rightW === 0 && 'right')
    : undefined;

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

  const handleOpenPreferences = useCallback(() => {
    setHelpOpen(false);
    setPreferencesOpen(true);
  }, []);

  const handleOpenHelp = useCallback(() => {
    setPreferencesOpen(false);
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
        case 'open-project':
          welcome.onOpenProject();
          break;
        case 'save-project':
          if (doc.hasDocument) onSaveProject();
          break;
        case 'save-project-as':
          if (doc.hasDocument) onSaveProjectAs();
          break;
        case 'share-project-copy':
          if (doc.hasDocument) onShareProjectCopy();
          break;
        case 'save-export':
          if (doc.hasDocument) onSaveImage();
          break;
        case 'export-ascii':
          if (doc.hasDocument) void doc.exportAscii();
          break;
        case 'undo':
          if (canUndo && doc.docId != null) void dispatch(undoDocument(doc.docId));
          break;
        case 'redo':
          if (canRedo && doc.docId != null) void dispatch(redoDocument(doc.docId));
          break;
        case 'copy-ascii-text':
          if (doc.hasDocument) void doc.copyAsciiText('txt');
          break;
        case 'copy-ascii-ansi':
          if (doc.hasDocument) void doc.copyAsciiText('ansi');
          break;
        case 'export-pattern':
          if (doc.hasDocument) void doc.exportPattern();
          break;
        case 'import-pattern':
          if (doc.hasDocument) void doc.importPattern();
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
    handleOpenHelp,
    handleOpenPreferences,
    onSaveImage,
    onSaveProject,
    onSaveProjectAs,
    onShareProjectCopy,
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

  const handleSwapSidebars = useCallback(() => {
    try {
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
              onSaveImage={onSaveImage}
              onExportAscii={() => void doc.exportAscii()}
              onCopyAsciiText={() => void doc.copyAsciiText('txt')}
              onCopyAsciiAnsi={() => void doc.copyAsciiText('ansi')}
              onOpenProject={welcome.onOpenProject}
              onOpenRecent={welcome.onOpenRecent}
              onSaveProject={onSaveProject}
              onSaveProjectAs={onSaveProjectAs}
              onShareProjectCopy={onShareProjectCopy}
              onExportPattern={() => void doc.exportPattern()}
              onImportPattern={() => void doc.importPattern()}
              onApplyCrossStitch={applyCrossStitchPreset}
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
                data-dock-side="left"
                data-app-edges={
                  leftFlexOnly && !leftSidebar.collapsed ? leftAppEdges : undefined
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
                <FlexLayoutContainer
                  side="left"
                  appEdges={leftFlexOnly && !leftSidebar.collapsed ? leftAppEdges : undefined}
                  style={{ height: '100%', minWidth: 0 }}
                />
              </div>
            </>
          )}
        </>
      )}

      <div
        className={cn('app-canvas')}
        data-panel-id="preview"
        data-app-edges={previewAppEdges}
        style={previewBackgroundStyle(previewBackground)}
      >
        {/* Keep center FlexLayout mounted while floated so the popout portal stays alive. */}
        <div
          style={
            previewFloating
              ? {
                  position: 'fixed',
                  width: 0,
                  height: 0,
                  overflow: 'hidden',
                  pointerEvents: 'none',
                  opacity: 0,
                }
              : { width: '100%', height: '100%', minHeight: 0 }
          }
          aria-hidden={previewFloating || undefined}
        >
          <FlexLayoutContainer side="center" welcome={welcome} appEdges={previewAppEdges} />
        </div>
        {previewFloating && (
          <div className={cn('preview-undocked-placeholder')} style={previewBackgroundStyle(previewBackground)}>
            <span>Preview is in a separate window</span>
          </div>
        )}
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
                data-dock-side="right"
                data-app-edges={
                  rightFlexOnly && !rightSidebar.collapsed ? rightAppEdges : undefined
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
                <FlexLayoutContainer
                  side="right"
                  appEdges={rightFlexOnly && !rightSidebar.collapsed ? rightAppEdges : undefined}
                  style={{ height: '100%', minWidth: 0 }}
                />
              </div>
            </>
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
