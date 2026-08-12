import { useCallback } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { clearNotification, openImage, saveImage } from '../app/slices/documentSlice';
import { refreshLayers } from '../app/slices/layersSlice';
import { refreshFilters } from '../app/slices/filtersSlice';
import { maybeAutoExtractPalette } from '../app/autoExtract';
import { useShell } from '../app/shell/ShellContext';
import { openDialog, saveDialog } from '../shared/ipc';

/**
 * Document open/save flows backed by RTK `document` slice.
 */
export function useDocument() {
  const dispatch = useAppDispatch();
  const state = useAppSelector((s) => s.document);
  const { autoExtractPalettes } = useShell();

  const openImageFn = useCallback(async () => {
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      });

      if (!filePath) return;

      const result = await dispatch(openImage(filePath as string));
      if (openImage.fulfilled.match(result)) {
        await dispatch(refreshLayers(result.payload.docId));
        await dispatch(refreshFilters());
        // Primary raster layer from load_image is always id 1
        const layerId = result.payload.layerId ?? 1;
        void maybeAutoExtractPalette(dispatch, layerId, autoExtractPalettes);
      }
    } catch {
      // Dialog cancel / IPC errors handled in thunk
    }
  }, [autoExtractPalettes, dispatch]);

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
    openImage: openImageFn,
    saveImage: saveImageFn,
    clearNotification: clearNotificationFn,
  };
}
