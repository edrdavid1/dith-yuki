import { useCallback } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import {
  clearNotification,
  createDocument,
  openImage,
  openProject,
  saveImage,
  saveProject,
  saveProjectAs,
  exportPattern,
  importPattern,
  setDocumentMeta,
} from '../app/slices/documentSlice';
import { refreshLayers } from '../app/slices/layersSlice';
import { refreshFilters } from '../app/slices/filtersSlice';
import { maybeAutoExtractPalette } from '../app/autoExtract';
import { useShell } from '../app/shell/ShellContext';
import { openDialog, saveDialog } from '../shared/ipc';
import type { BlankBackground } from '../shared/ipc/document';

/**
 * Document open/save flows backed by RTK `document` slice.
 */
export function useDocument() {
  const dispatch = useAppDispatch();
  const state = useAppSelector((s) => s.document);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const { autoExtractPalettes } = useShell();

  const openImageAtFn = useCallback(
    async (filePath: string) => {
      const result = await dispatch(openImage(filePath));
      if (openImage.fulfilled.match(result)) {
        await dispatch(refreshLayers(result.payload.docId));
        await dispatch(refreshFilters());
        const layerId = result.payload.layerId ?? 1;
        void maybeAutoExtractPalette(dispatch, layerId, autoExtractPalettes);
      }
    },
    [autoExtractPalettes, dispatch]
  );

  const openImageFn = useCallback(async () => {
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      });

      if (!filePath) return;

      await openImageAtFn(filePath as string);
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [openImageAtFn]);

  const saveImageFn = useCallback(async () => {
    if (!state.docId) return;

    try {
      const filePath = await saveDialog({
        filters: [
          { name: 'PNG', extensions: ['png'] },
          { name: 'JPEG', extensions: ['jpg', 'jpeg'] },
          { name: 'SVG', extensions: ['svg'] },
        ],
      });

      if (!filePath) return;

      const lower = filePath.toLowerCase();
      const format =
        lower.endsWith('.jpg') || lower.endsWith('.jpeg')
          ? ('JPEG' as const)
          : lower.endsWith('.svg')
            ? ('SVG' as const)
            : ('PNG' as const);

      const filename = filePath.split(/[/\\]/).pop() ?? filePath;
      await dispatch(
        saveImage({
          doc_id: state.docId,
          path: filePath,
          format,
          quality: format === 'JPEG' ? 90 : undefined,
          filename,
        })
      );
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [dispatch, state.docId]);

  const openProjectAtFn = useCallback(
    async (filePath: string) => {
      const result = await dispatch(openProject(filePath));
      if (openProject.fulfilled.match(result)) {
        await dispatch(refreshLayers(result.payload.docId));
        await dispatch(refreshFilters());
      }
    },
    [dispatch]
  );

  const openProjectFn = useCallback(async () => {
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
      });
      if (!filePath) return;

      await openProjectAtFn(filePath as string);
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [openProjectAtFn]);

  const saveProjectFn = useCallback(async () => {
    if (!state.hasDocument) return;
    try {
      if (state.projectPath) {
        await dispatch(saveProject(null));
        return;
      }
      const filePath = await saveDialog({
        filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
      });
      if (!filePath) return;
      const path = filePath.toLowerCase().endsWith('.dyproj')
        ? filePath
        : `${filePath}.dyproj`;
      await dispatch(saveProjectAs(path));
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [dispatch, state.hasDocument, state.projectPath]);

  const saveProjectAsFn = useCallback(async () => {
    if (!state.hasDocument) return;
    try {
      const filePath = await saveDialog({
        filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
      });
      if (!filePath) return;
      const path = filePath.toLowerCase().endsWith('.dyproj')
        ? filePath
        : `${filePath}.dyproj`;
      await dispatch(saveProjectAs(path));
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [dispatch, state.hasDocument]);

  const exportPatternFn = useCallback(
    async (layerId?: number | null) => {
      const target = layerId ?? selectedLayerId;
      if (target == null) {
        dispatch(setDocumentMeta({ error: 'Select a layer to export a pattern' }));
        return;
      }
      try {
        const filePath = await saveDialog({
          filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
        });
        if (!filePath) return;
        const path = filePath.toLowerCase().endsWith('.dyuki')
          ? filePath
          : `${filePath}.dyuki`;
        await dispatch(exportPattern({ layerId: target, path }));
      } catch {
        // Dialog cancel / IPC errors handled in thunk
      }
    },
    [dispatch, selectedLayerId]
  );

  const importPatternFn = useCallback(
    async (layerId?: number | null) => {
      const target = layerId ?? selectedLayerId;
      if (target == null) {
        dispatch(setDocumentMeta({ error: 'Select a layer to import a pattern' }));
        return;
      }
      try {
        const filePath = await openDialog({
          multiple: false,
          filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
        });
        if (!filePath) return;
        const result = await dispatch(
          importPattern({ path: filePath as string, targetLayerId: target })
        );
        if (importPattern.fulfilled.match(result)) {
          await dispatch(refreshLayers(state.docId));
          await dispatch(refreshFilters());
        }
      } catch {
        // Dialog cancel / IPC errors handled in thunk
      }
    },
    [dispatch, selectedLayerId, state.docId]
  );

  const createDocumentFn = useCallback(
    async (args: { width: number; height: number; background: BlankBackground }) => {
      const result = await dispatch(createDocument(args));
      if (createDocument.fulfilled.match(result)) {
        await dispatch(refreshLayers(result.payload.docId));
        await dispatch(refreshFilters());
        return true;
      }
      return false;
    },
    [dispatch]
  );

  const clearNotificationFn = useCallback(() => {
    dispatch(clearNotification());
  }, [dispatch]);

  return {
    docId: state.docId,
    width: state.width,
    height: state.height,
    layerId: state.layerId,
    loading: state.loading,
    error: state.error,
    notification: state.notification,
    hasDocument: state.hasDocument,
    projectPath: state.projectPath,
    openImage: openImageFn,
    openImageAt: openImageAtFn,
    saveImage: saveImageFn,
    openProject: openProjectFn,
    openProjectAt: openProjectAtFn,
    saveProject: saveProjectFn,
    saveProjectAs: saveProjectAsFn,
    createDocument: createDocumentFn,
    exportPattern: exportPatternFn,
    importPattern: importPatternFn,
    clearNotification: clearNotificationFn,
  };
}
