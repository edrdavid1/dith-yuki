import { useEffect } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { onNativeMenu } from '../shared/ipc';
import { redo as redoDocument, undo as undoDocument } from './slices/undoSlice';
import { useAppDispatch } from './hooks';
import {
  registerDocumentCommands,
  registerLayoutCommands,
} from '../features/shortcuts/commandRegistry';

type SessionDoc = {
  hasDocument: boolean;
  docId: number | null;
  exportAscii: () => unknown;
  copyAsciiText: (format: 'txt' | 'ansi') => unknown;
  exportPattern: () => unknown;
  importPattern: () => unknown;
  copyPreviewImage: () => unknown;
};

/**
 * Native menu items and the shortcut command registry for the main window.
 */
export function useSessionCommands(args: {
  welcome: {
    onNewProject: () => void;
    onOpenImage: () => void;
    onOpenProject: () => void;
  };
  doc: SessionDoc;
  canUndo: boolean;
  canRedo: boolean;
  onSaveProject: () => void;
  onSaveProjectAs: () => void;
  onShareProjectCopy: () => void;
  onSaveImage: () => void;
  onBatchExport: () => void;
  handleOpenPreferences: () => void;
  handleOpenHelp: () => void;
  returnAllPanelsToDock: () => void;
  requestQuit: () => Promise<void>;
  checkForUpdates: () => unknown;
  directChannel: boolean;
  setFocusMode: Dispatch<SetStateAction<boolean>>;
  handleSwapSidebars: () => void;
}): void {
  const dispatch = useAppDispatch();
  const {
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
    checkForUpdates,
    directChannel,
    setFocusMode,
    handleSwapSidebars,
  } = args;

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
        case 'batch-export':
          if (doc.hasDocument) onBatchExport();
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
        case 'return-all-panels-to-dock':
          returnAllPanelsToDock();
          break;
        case 'about':
        case 'help':
          handleOpenHelp();
          break;
        case 'help-check-updates':
        case 'check-updates':
          if (directChannel) void checkForUpdates();
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
    checkForUpdates,
    directChannel,
    dispatch,
    doc,
    handleOpenHelp,
    handleOpenPreferences,
    onBatchExport,
    onSaveImage,
    onSaveProject,
    onSaveProjectAs,
    onShareProjectCopy,
    requestQuit,
    returnAllPanelsToDock,
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
      swapSidebars: () => void handleSwapSidebars(),
      copyPreviewImage: () => {
        if (doc.hasDocument) void doc.copyPreviewImage();
      },
    });
  }, [doc.copyPreviewImage, doc.hasDocument, handleSwapSidebars, setFocusMode]);
}
