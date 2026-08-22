import { useCallback, useRef, useState } from 'react';
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
  importImageLayer,
  setDocumentMeta,
} from '../app/slices/documentSlice';
import { refreshLayers } from '../app/slices/layersSlice';
import { refreshFilters } from '../app/slices/filtersSlice';
import { maybeAutoExtractPalette } from '../app/autoExtract';
import { useShell } from '../app/shell/ShellContext';
import { openDialog, saveDialog } from '../shared/ipc';
import { listOpenDocuments } from '../shared/ipc/document';
import type { BlankBackground } from '../shared/ipc/document';
import {
  OPEN_DOC_MEMORY_WARNING,
  shouldWarnOpenDocMemory,
} from '../shared/memoryWarning';
import SvgExportDialog, { type SvgExportAlgorithm } from '../components/SvgExportDialog';

/**
 * Document open/save flows backed by RTK `document` slice.
 */
export function useDocument() {
  const dispatch = useAppDispatch();
  const state = useAppSelector((s) => s.document);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const { autoExtractPalettes } = useShell();
  const [svgOpen, setSvgOpen] = useState(false);
  const svgResolver = useRef<((algo: SvgExportAlgorithm | null) => void) | null>(null);

  const maybeWarnMemory = useCallback(async () => {
    try {
      const { tabs } = await listOpenDocuments();
      if (shouldWarnOpenDocMemory(tabs.length)) {
        dispatch(setDocumentMeta({ notification: OPEN_DOC_MEMORY_WARNING }));
      }
    } catch {
      // non-fatal
    }
  }, [dispatch]);

  const openImageAtFn = useCallback(
    async (filePath: string) => {
      const result = await dispatch(openImage(filePath));
      if (openImage.fulfilled.match(result)) {
        await dispatch(refreshLayers(result.payload.docId));
        await dispatch(refreshFilters());
        const layerId = result.payload.layerId ?? 1;
        void maybeAutoExtractPalette(dispatch, layerId, autoExtractPalettes);
        void maybeWarnMemory();
      }
    },
    [autoExtractPalettes, dispatch, maybeWarnMemory]
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
      let svg_algorithm: SvgExportAlgorithm | undefined;
      if (format === 'SVG') {
        const picked = await new Promise<SvgExportAlgorithm | null>((resolve) => {
          svgResolver.current = resolve;
          setSvgOpen(true);
        });
        setSvgOpen(false);
        svgResolver.current = null;
        if (!picked) return;
        svg_algorithm = picked;
      }
      await dispatch(
        saveImage({
          doc_id: state.docId,
          path: filePath,
          format,
          quality: format === 'JPEG' ? 90 : undefined,
          filename,
          svg_algorithm,
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
        void maybeWarnMemory();
      }
    },
    [dispatch, maybeWarnMemory]
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

  const saveProjectFn = useCallback(async (): Promise<boolean> => {
    if (!state.hasDocument || state.docId == null) return false;
    const docId = state.docId;
    try {
      if (state.projectPath) {
        const result = await dispatch(saveProject({ docId, path: null }));
        return saveProject.fulfilled.match(result);
      }
      const filePath = await saveDialog({
        filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
      });
      if (!filePath) return false;
      const path = filePath.toLowerCase().endsWith('.dyproj')
        ? filePath
        : `${filePath}.dyproj`;
      const result = await dispatch(saveProjectAs({ docId, path }));
      return saveProjectAs.fulfilled.match(result);
    } catch {
      return false;
    }
  }, [dispatch, state.docId, state.hasDocument, state.projectPath]);

  const saveProjectAsFn = useCallback(async () => {
    if (!state.hasDocument || state.docId == null) return;
    const docId = state.docId;
    try {
      const filePath = await saveDialog({
        filters: [{ name: 'Dither Project', extensions: ['dyproj'] }],
      });
      if (!filePath) return;
      const path = filePath.toLowerCase().endsWith('.dyproj')
        ? filePath
        : `${filePath}.dyproj`;
      await dispatch(saveProjectAs({ docId, path }));
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [dispatch, state.docId, state.hasDocument]);

  const exportPatternFn = useCallback(
    async (layerId?: number | null) => {
      const target = layerId ?? selectedLayerId;
      if (target == null || state.docId == null) {
        dispatch(setDocumentMeta({ error: 'Select a layer to export a pattern' }));
        return;
      }
      const docId = state.docId;
      try {
        const filePath = await saveDialog({
          filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
        });
        if (!filePath) return;
        const path = filePath.toLowerCase().endsWith('.dyuki')
          ? filePath
          : `${filePath}.dyuki`;
        await dispatch(exportPattern({ docId, layerId: target, path }));
      } catch {
        // Dialog cancel / IPC errors handled in thunk
      }
    },
    [dispatch, selectedLayerId, state.docId]
  );

  const importPatternFn = useCallback(
    async (layerId?: number | null) => {
      const target = layerId ?? selectedLayerId;
      if (target == null || state.docId == null) {
        dispatch(setDocumentMeta({ error: 'Select a layer to import a pattern' }));
        return;
      }
      const docId = state.docId;
      try {
        const filePath = await openDialog({
          multiple: false,
          filters: [{ name: 'Dither Pattern', extensions: ['dyuki'] }],
        });
        if (!filePath) return;
        const result = await dispatch(
          importPattern({ docId, path: filePath as string, targetLayerId: target })
        );
        if (importPattern.fulfilled.match(result)) {
          await dispatch(refreshLayers(docId));
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
        void maybeWarnMemory();
        return true;
      }
      return false;
    },
    [dispatch, maybeWarnMemory]
  );

  const importImageLayerFn = useCallback(async () => {
    if (!state.hasDocument || state.docId == null) return;
    const docId = state.docId;
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      });
      if (!filePath) return;
      const result = await dispatch(importImageLayer({ docId, path: filePath as string }));
      if (importImageLayer.fulfilled.match(result)) {
        await dispatch(refreshLayers(docId));
        await dispatch(refreshFilters());
        void maybeAutoExtractPalette(dispatch, result.payload.layerId, autoExtractPalettes);
      }
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [autoExtractPalettes, dispatch, state.docId, state.hasDocument]);

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
    sourcePath: state.sourcePath,
    dirty: state.dirty,
    openImage: openImageFn,
    openImageAt: openImageAtFn,
    saveImage: saveImageFn,
    openProject: openProjectFn,
    openProjectAt: openProjectAtFn,
    saveProject: saveProjectFn,
    saveProjectAs: saveProjectAsFn,
    createDocument: createDocumentFn,
    importImageLayer: importImageLayerFn,
    exportPattern: exportPatternFn,
    importPattern: importPatternFn,
    clearNotification: clearNotificationFn,
    svgDialog: (
      <SvgExportDialog
        isOpen={svgOpen}
        onExport={(algo) => svgResolver.current?.(algo)}
        onClose={() => svgResolver.current?.(null)}
      />
    ),
  };
}
