import { useCallback, useEffect, useState } from 'react';
import MenuBar from '../components/MenuBar';
import DocumentTabBar from '../features/document/DocumentTabBar';
import { WindowShell } from '../components/WindowShell';
import Notification, { BusyNotification } from '../components/common/Notification';
import NewProjectDialog from '../components/NewProjectDialog';
import HelpDialog from '../components/HelpDialog';
import PreferencesDialog from '../features/preferences/PreferencesDialog';
import PatternManagerDialog from '../features/patterns/PatternManagerDialog';
import { usePatternsUi } from '../features/patterns/PatternsUiContext';
import WorkspacePromptDialog from '../features/workspace/WorkspacePromptDialog';
import WorkspaceHintBanner from '../features/workspace/WorkspaceHintBanner';
import { useWorkspaceOptional } from '../features/workspace/WorkspaceContext';
import { useWelcomeScreen } from '../hooks/useWelcomeScreen';
import { ChannelGates, IS_DIRECT } from '../channel';
import { useAppUpdates } from '../hooks/useAppUpdates';
import { useAppDispatch, useAppSelector } from './hooks';
import { addLayerWithPreset } from './slices/layersSlice';
import { setDocumentMeta } from './slices/documentSlice';
import { setSelection } from './slices/selectionSlice';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import SidebarCollapseStrip from '../features/panels/SidebarCollapseStrip';
import FlexLayoutContainer from '../components/FlexLayoutContainer';
import ResizeHandle from '../components/common/ResizeHandle';
import styles from './AppLayout.module.css';
import menuStyles from '../features/document/MenuBar.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import previewWindowStyles from '../features/preview/PreviewWindow.module.css';
import resizeStyles from '../shared/ui/ResizeHandle.module.css';
import { windowChromeTitle } from '../shared/windowTitle';
import { isTooNewFileError } from '../shared/appUpdates';
import { bind } from '../shared/ui/cn';
import Tooltip from '../shared/ui/Tooltip';
import Icon from '../icons/iconRegistry';
import { labelWithShortcut } from '../features/shortcuts/bindings';
import { useShortcutBindings } from '../features/shortcuts/ShortcutsContext';
import { useMainWindowLifetime } from './useMainWindowLifetime';
import { useDockFrame } from './useDockFrame';
import { useSessionCommands } from './useSessionCommands';

const cn = bind({ ...styles, ...menuStyles, ...previewStyles, ...previewWindowStyles, ...resizeStyles });

/**
 * Main shell layout: menubar + optional left sidebar | canvas | optional right sidebar.
 * Quit, dock geometry, and native-menu commands live in the hooks beside this file.
 */
export default function AppLayout() {
  const dispatch = useAppDispatch();
  const workspaceApi = useWorkspaceOptional();
  const workspace = workspaceApi?.workspace ?? 'general';
  const {
    doc,
    welcome,
    newProjectOpen,
    closeNewProject,
    handleCreate,
    onSaveImage,
    onExportPrint,
    onBatchExport,
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
    autoCheckOnLaunch: IS_DIRECT,
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

  const { requestQuit } = useMainWindowLifetime(doc, workspace, confirmQuit);
  const projectPath = doc.projectPath;
  const sourcePath = doc.sourcePath;

  useEffect(() => {
    const name = projectPath || sourcePath;
    if (!name || !workspaceApi) return;
    void workspaceApi.triggerHint({
      trigger: 'filename',
      filename: name.split(/[/\\]/).pop() ?? name,
    });
  }, [projectPath, sourcePath, workspaceApi]);

  const [dismissedError, setDismissedError] = useState<string | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const {
    isOpen: patternsOpen,
    intent: patternsIntent,
    intentNonce: patternsIntentNonce,
    openPatternManager,
    closePatternManager,
  } = usePatternsUi();
  const [focusMode, setFocusMode] = useState(false);

  const {
    affinity,
    leftHitRef,
    rightHitRef,
    leftDockedFlexIds,
    rightDockedFlexIds,
    previewFloating,
    leftFlexOnly,
    rightFlexOnly,
    leftFloatHostOnly,
    rightFloatHostOnly,
    leftW,
    rightW,
    leftVisuallyCollapsed,
    rightVisuallyCollapsed,
    leftAppEdges,
    rightAppEdges,
    previewAppEdges,
    previewBackground,
    setSidebarCollapsed,
    swapSidebars,
    swapFlexSides,
    layoutToast,
    layoutToastEpoch,
    clearLayoutToast,
    dockPanel,
    returnAllPanelsToDock,
    resizeColumnWidth,
    hasFloatingPanels,
    gridTemplateColumns,
  } = useDockFrame(focusMode);

  const handleOpenPreferences = useCallback(() => {
    setHelpOpen(false);
    closePatternManager();
    setPreferencesOpen(true);
  }, [closePatternManager]);

  const handleOpenPatterns = useCallback(() => {
    setHelpOpen(false);
    setPreferencesOpen(false);
    openPatternManager('browse');
  }, [openPatternManager]);

  const handleOpenHelp = useCallback(() => {
    setPreferencesOpen(false);
    closePatternManager();
    setHelpOpen(true);
  }, [closePatternManager]);

  const handleSwapSidebars = useCallback(() => {
    try {
      swapSidebars();
      swapFlexSides();
    } catch (err) {
      console.error('Swap sidebars failed:', err);
    }
  }, [swapSidebars, swapFlexSides]);

  useSessionCommands({
    welcome,
    doc,
    canUndo,
    canRedo,
    onSaveProject,
    onSaveProjectAs,
    onShareProjectCopy,
    onSaveImage,
    onBatchExport,
    handleOpenPreferences,
    handleOpenHelp,
    returnAllPanelsToDock,
    requestQuit,
    checkForUpdates: updates.checkForUpdates,
    directChannel: IS_DIRECT,
    setFocusMode,
    handleSwapSidebars,
  });

  const shortcuts = useShortcutBindings();

  const currentError = doc.error || layersError || filtersError;
  const toastError =
    currentError && !isTooNewFileError(currentError) ? currentError : null;
  const displayError = toastError && toastError !== dismissedError ? toastError : null;
  const openingToast =
    doc.loading && doc.hasDocument ? 'Opening…' : null;

  return (
    <WindowShell
      titlebarContent={
        <>
          <div data-tauri-drag-region="false">
            <MenuBar
              hasDocument={doc.hasDocument}
              opening={doc.loading}
              canUndo={canUndo}
              canRedo={canRedo}
              recentEntries={welcome.recentEntries}
              onNewProject={welcome.onNewProject}
              onOpenImage={welcome.onOpenImage}
              onSaveImage={onSaveImage}
              onExportPrint={onExportPrint}
              onExportAscii={() => void doc.exportAscii()}
              onBatchExport={onBatchExport}
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
              onOpenPatterns={handleOpenPatterns}
              onOpenPreferences={handleOpenPreferences}
              onOpenHelp={handleOpenHelp}
              onReturnAllPanelsToDock={returnAllPanelsToDock}
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
            workspace,
          })}</div>
          <div className={cn('toolbar-icon-group')} data-tauri-drag-region="false">
            <Tooltip
              label={labelWithShortcut(
                'Swap left and right sidebars',
                shortcuts.swapSidebars
              )}
            >
              <button
                type="button"
                className={cn('sidebar-changer-icon-btn')}
                onClick={() => void handleSwapSidebars()}
                aria-label={labelWithShortcut(
                  'Swap left and right sidebars',
                  shortcuts.swapSidebars
                )}
              >
                <Icon name="sidebar-swap" width={16} height={16} />
              </button>
            </Tooltip>
            <Tooltip
              label={labelWithShortcut(
                focusMode ? 'Exit focus mode' : 'Focus mode — hide sidebars',
                shortcuts.focusMode
              )}
            >
              <button
                type="button"
                className={cn(
                  'sidebar-changer-icon-btn',
                  focusMode && 'sidebar-changer-icon-btn-active'
                )}
                onClick={() => setFocusMode((on) => !on)}
                aria-label={labelWithShortcut(
                  focusMode ? 'Exit focus mode' : 'Focus mode',
                  shortcuts.focusMode
                )}
                aria-pressed={focusMode}
              >
                <Icon name="focus-mode" width={16} height={16} />
              </button>
            </Tooltip>
            <Tooltip label="Return all panels to dock">
              <button
                type="button"
                className={cn('sidebar-changer-icon-btn')}
                onClick={() => returnAllPanelsToDock()}
                disabled={!hasFloatingPanels}
                aria-label="Return all panels to dock"
              >
                <Icon name="return-panels" width={16} height={16} />
              </button>
            </Tooltip>
            <Tooltip
              label={labelWithShortcut(
                'Copy preview image as PNG',
                shortcuts.copyPreview
              )}
            >
              <button
                type="button"
                className={cn('sidebar-changer-icon-btn')}
                onClick={() => {
                  if (doc.hasDocument) void doc.copyPreviewImage();
                }}
                disabled={!doc.hasDocument}
                aria-label={labelWithShortcut(
                  'Copy preview image as PNG',
                  shortcuts.copyPreview
                )}
              >
                <Icon name="copy-x" width={16} height={16} />
              </button>
            </Tooltip>
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
              {leftFlexOnly && leftVisuallyCollapsed && (
                <SidebarCollapseStrip
                  side="left"
                  panelIds={leftDockedFlexIds}
                  onExpand={() => setSidebarCollapsed('left', false)}
                  hitTargetRef={leftHitRef}
                />
              )}
              {leftFlexOnly && !leftVisuallyCollapsed && (
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
              {/* Float-host: invisible drop rail only (no Return strip).
                  Return via popout close / header “Return all panels” button. */}
              {leftFloatHostOnly && (
                <div
                  ref={(el) => {
                    leftHitRef.current = el;
                  }}
                  className={cn(
                    'sidebar-empty-drop-edge',
                    'sidebar-empty-drop-edge-left',
                    affinity?.armed &&
                      affinity.side === 'left' &&
                      'sidebar-empty-drop-edge-armed',
                  )}
                  data-dock-empty-edge="left"
                  aria-hidden
                />
              )}
              <div
                className={
                  leftFlexOnly && !leftVisuallyCollapsed
                    ? cn('app-sidebar', 'sidebar-area-left')
                    : undefined
                }
                data-dock-side="left"
                data-app-edges={
                  leftFlexOnly && !leftVisuallyCollapsed ? leftAppEdges : undefined
                }
                style={
                  leftFlexOnly && !leftVisuallyCollapsed
                    ? { minWidth: leftW, minHeight: 0, height: '100%' }
                    : {
                        position: 'fixed',
                        width: 0,
                        height: 0,
                        overflow: 'hidden',
                        pointerEvents: 'none',
                        opacity: 0,
                      }
                }
                aria-hidden={!(leftFlexOnly && !leftVisuallyCollapsed)}
              >
                <FlexLayoutContainer
                  side="left"
                  appEdges={leftFlexOnly && !leftVisuallyCollapsed ? leftAppEdges : undefined}
                  style={{ height: '100%', minWidth: leftW }}
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
            <button
              type="button"
              className={cn('preview-undocked-return-btn')}
              onClick={() => dockPanel('center', 'preview')}
            >
              Return to canvas
            </button>
          </div>
        )}
      </div>

      {!focusMode && (
        <>
          {/* Right flex column — stable FlexLayout mount (see left). */}
          {(rightFlexOnly || rightFloatHostOnly) && (
            <>
              {rightFlexOnly && rightVisuallyCollapsed && (
                <SidebarCollapseStrip
                  side="right"
                  panelIds={rightDockedFlexIds}
                  onExpand={() => setSidebarCollapsed('right', false)}
                  hitTargetRef={rightHitRef}
                />
              )}
              {rightFlexOnly && !rightVisuallyCollapsed && (
                <ResizeHandle
                  direction="horizontal"
                  onResize={(d) => resizeColumnWidth('right', d, true)}
                  className={cn('sidebar-resize-handle', 'sidebar-resize-right')}
                />
              )}
              {rightFloatHostOnly && (
                <div
                  ref={(el) => {
                    rightHitRef.current = el;
                  }}
                  className={cn(
                    'sidebar-empty-drop-edge',
                    'sidebar-empty-drop-edge-right',
                    affinity?.armed &&
                      affinity.side === 'right' &&
                      'sidebar-empty-drop-edge-armed',
                  )}
                  data-dock-empty-edge="right"
                  aria-hidden
                />
              )}
              <div
                className={
                  rightFlexOnly && !rightVisuallyCollapsed
                    ? cn('app-sidebar', 'sidebar-area-right')
                    : undefined
                }
                data-dock-side="right"
                data-app-edges={
                  rightFlexOnly && !rightVisuallyCollapsed ? rightAppEdges : undefined
                }
                style={
                  rightFlexOnly && !rightVisuallyCollapsed
                    ? { minWidth: rightW, minHeight: 0, height: '100%' }
                    : {
                        position: 'fixed',
                        width: 0,
                        height: 0,
                        overflow: 'hidden',
                        pointerEvents: 'none',
                        opacity: 0,
                      }
                }
                aria-hidden={!(rightFlexOnly && !rightVisuallyCollapsed)}
              >
                <FlexLayoutContainer
                  side="right"
                  appEdges={rightFlexOnly && !rightVisuallyCollapsed ? rightAppEdges : undefined}
                  style={{ height: '100%', minWidth: rightW }}
                />
              </div>
            </>
          )}
        </>
      )}

      <BusyNotification message={openingToast} />
      <WorkspaceHintBanner />
      <Notification
        message={displayError}
        onDismiss={() => setDismissedError(currentError)}
      />
      <Notification
        message={doc.notification}
        showId={doc.notificationEpoch}
        onDismiss={doc.clearNotification}
      />
      <Notification
        message={layoutToast}
        showId={layoutToastEpoch}
        onDismiss={clearLayoutToast}
      />
      <NewProjectDialog
        isOpen={newProjectOpen}
        onClose={closeNewProject}
        onCreate={handleCreate}
      />
      <PreferencesDialog isOpen={preferencesOpen} onClose={() => setPreferencesOpen(false)} />
      <WorkspacePromptDialog />
      <PatternManagerDialog
        isOpen={patternsOpen}
        intent={patternsIntent}
        intentNonce={patternsIntentNonce}
        onClose={closePatternManager}
      />
      <HelpDialog
        isOpen={helpOpen}
        version={updates.version}
        checking={updates.checking}
        onClose={() => setHelpOpen(false)}
        onCheckForUpdates={
          IS_DIRECT ? () => void updates.checkForUpdates() : undefined
        }
      />
      {unsavedDialog}
      {IS_DIRECT ? updates.dialogs : null}
      <ChannelGates />
      {doc.svgDialog}
    </div>
    </WindowShell>
  );
}
