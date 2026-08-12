import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import {
  formatIpcError,
  getDocumentSnapshot,
  loadImage as loadImageIPC,
  exportImage as exportImageIPC,
  logIpcError,
} from '../../shared/ipc';
import type { ExportImageRequest } from '../../types';

export interface DocumentState {
  docId: number | null;
  width: number;
  height: number;
  hasDocument: boolean;
  loading: boolean;
  notification: string | null;
  error: string | null;
  layerId: number | null;
}

const initialState: DocumentState = {
  docId: null,
  width: 0,
  height: 0,
  hasDocument: false,
  loading: false,
  notification: null,
  error: null,
  layerId: null,
};

export const refreshDocument = createAsyncThunk(
  'document/refresh',
  async (_, { rejectWithValue }) => {
    try {
      const response = await getDocumentSnapshot();
      const snap = response.snapshot;
      return {
        docId: snap.id ?? null,
        width: snap.width ?? 0,
        height: snap.height ?? 0,
        hasDocument: (snap.layers?.length ?? 0) > 0,
      };
    } catch (err) {
      logIpcError('document.refresh', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const openImage = createAsyncThunk(
  'document/openImage',
  async (path: string, { rejectWithValue }) => {
    try {
      const response = await loadImageIPC(path);
      return {
        docId: response.doc_id,
        width: response.width,
        height: response.height,
        layerId: 1,
      };
    } catch (err) {
      logIpcError('document.openImage', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const saveImage = createAsyncThunk(
  'document/saveImage',
  async (req: ExportImageRequest & { filename: string }, { rejectWithValue }) => {
    try {
      await exportImageIPC(req);
      return `Saved: ${req.filename}`;
    } catch (err) {
      logIpcError('document.saveImage', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

const documentSlice = createSlice({
  name: 'document',
  initialState,
  reducers: {
    clearNotification(state) {
      state.notification = null;
    },
    clearError(state) {
      state.error = null;
    },
    setDocumentMeta(
      state,
      action: PayloadAction<Partial<Pick<DocumentState, 'docId' | 'width' | 'height' | 'hasDocument' | 'layerId' | 'error' | 'notification' | 'loading'>>>
    ) {
      Object.assign(state, action.payload);
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(refreshDocument.fulfilled, (state, action) => {
        state.docId = action.payload.docId;
        state.width = action.payload.width;
        state.height = action.payload.height;
        state.hasDocument = action.payload.hasDocument;
        state.error = null;
      })
      .addCase(refreshDocument.rejected, (state, action) => {
        state.docId = null;
        state.width = 0;
        state.height = 0;
        state.hasDocument = false;
        state.error = (action.payload as string) ?? 'Failed to refresh document';
      })
      .addCase(openImage.pending, (state) => {
        state.loading = true;
        state.error = null;
        state.notification = null;
      })
      .addCase(openImage.fulfilled, (state, action) => {
        state.loading = false;
        state.docId = action.payload.docId;
        state.width = action.payload.width;
        state.height = action.payload.height;
        state.layerId = action.payload.layerId;
        state.hasDocument = true;
        state.error = null;
      })
      .addCase(openImage.rejected, (state, action) => {
        state.loading = false;
        state.error = (action.payload as string) ?? 'Failed to open image';
      })
      .addCase(saveImage.fulfilled, (state, action) => {
        state.notification = action.payload;
        state.error = null;
      })
      .addCase(saveImage.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to save image';
        state.notification = null;
      });
  },
});

export const { clearNotification, clearError, setDocumentMeta } = documentSlice.actions;
export default documentSlice.reducer;
