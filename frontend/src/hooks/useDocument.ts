import { useState, useCallback } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { loadImage, exportImage } from '../ipc/commands';

interface DocumentState {
  docId: number | null;
  width: number;
  height: number;
  layerId: number | null;
  loading: boolean;
  error: string | null;
  notification: string | null;
}

export function useDocument() {
  const [state, setState] = useState<DocumentState>({
    docId: null,
    width: 0,
    height: 0,
    layerId: null,
    loading: false,
    error: null,
    notification: null,
  });

  const openImage = useCallback(async () => {
    try {
      const filePath = await open({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      });

      if (!filePath) return; // User cancelled

      setState(prev => ({ ...prev, loading: true, error: null, notification: null }));

      const response = await loadImage(filePath as string);

      setState({
        docId: response.doc_id,
        width: response.width,
        height: response.height,
        layerId: 1, // The base raster layer created by load_image
        loading: false,
        error: null,
        notification: null,
      });
    } catch (err) {
      setState(prev => ({
        ...prev,
        loading: false,
        error: typeof err === 'string' ? err : String(err),
      }));
    }
  }, []);

  const saveImage = useCallback(async () => {
    if (!state.docId) return;

    try {
      const filePath = await save({
        filters: [
          { name: 'PNG', extensions: ['png'] },
          { name: 'JPEG', extensions: ['jpg', 'jpeg'] },
        ],
      });

      if (!filePath) return; // User cancelled

      const format = filePath.toLowerCase().endsWith('.jpg') || filePath.toLowerCase().endsWith('.jpeg')
        ? 'JPEG' as const
        : 'PNG' as const;

      setState(prev => ({ ...prev, error: null, notification: null }));

      await exportImage({
        doc_id: state.docId,
        path: filePath,
        format,
        quality: format === 'JPEG' ? 90 : undefined,
      });

      // Extract filename from path for the success message
      const filename = filePath.split(/[/\\]/).pop() ?? filePath;
      setState(prev => ({
        ...prev,
        notification: `Saved: ${filename}`,
      }));
    } catch (err) {
      setState(prev => ({
        ...prev,
        error: typeof err === 'string' ? err : String(err),
        notification: null,
      }));
    }
  }, [state.docId]);

  const clearNotification = useCallback(() => {
    setState(prev => ({ ...prev, notification: null }));
  }, []);

  return {
    ...state,
    hasDocument: state.docId !== null,
    openImage,
    saveImage,
    clearNotification,
  };
}
