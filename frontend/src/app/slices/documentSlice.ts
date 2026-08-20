import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import {
  formatIpcError,
  getDocumentSnapshot,
  loadImage as loadImageIPC,
  createDocument as createDocumentIPC,
  exportImage as exportImageIPC,
  importImageLayer as importImageLayerIPC,
  openProject as openProjectIPC,
  saveProject as saveProjectIPC,
  saveProjectAs as saveProjectAsIPC,
  exportPattern as exportPatternIPC,
  importPattern as importPatternIPC,
  logIpcError,
} from '../../shared/ipc';
import type { ExportImageRequest } from '../../types';
import type { BlankBackground } from '../../shared/ipc/document';

export interface DocumentState {
  docId: number | null;
  width: number;
  height: number;
  hasDocument: boolean;
  /** False until the first `refreshDocument` (or open/create) settles. */
  hydrated: boolean;
  loading: boolean;
  notification: string | null;
  error: string | null;
  layerId: number | null;
  /** Remembered `.dyproj` path after Save As / Open Project (UI hint only). */
  projectPath: string | null;
  /** Last opened raster path, used in the window title until a project is saved. */
  sourcePath: string | null;
  /** Track P: diverges from last save / replace. */
  dirty: boolean;
  /** Bumps when the raster source is replaced while `docId` stays 1. */
  documentEpoch: number;
}

const initialState: DocumentState = {
  docId: null,
  width: 0,
  height: 0,
  hasDocument: false,
  hydrated: false,
  loading: false,
  notification: null,
  error: null,
  layerId: null,
  projectPath: null,
  sourcePath: null,
  dirty: false,
  documentEpoch: 0,
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

export const createDocument = createAsyncThunk(
  'document/createDocument',
  async (
    args: { width: number; height: number; background: BlankBackground },
    { rejectWithValue }
  ) => {
    try {
      const response = await createDocumentIPC(args.width, args.height, args.background);
      return {
        docId: response.doc_id,
        width: response.width,
        height: response.height,
        layerId: 1,
      };
    } catch (err) {
      logIpcError('document.createDocument', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const importImageLayer = createAsyncThunk(
  'document/importImageLayer',
  async (path: string, { rejectWithValue }) => {
    try {
      const response = await importImageLayerIPC(path);
      return { layerId: response.layer_id };
    } catch (err) {
      logIpcError('document.importImageLayer', err);
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

export const openProject = createAsyncThunk(
  'document/openProject',
  async (path: string, { rejectWithValue }) => {
    try {
      const response = await openProjectIPC(path);
      return {
        docId: response.doc_id,
        width: response.width,
        height: response.height,
        layerId: 1,
        projectPath: response.path,
      };
    } catch (err) {
      logIpcError('document.openProject', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const saveProject = createAsyncThunk(
  'document/saveProject',
  async (path: string | null | undefined, { rejectWithValue }) => {
    try {
      const response = await saveProjectIPC(path);
      let notification = `Project saved: ${response.path.split(/[/\\]/).pop() ?? response.path}`;
      if (response.size_warning) {
        notification += ' (large project — uncompressed layers exceed 256 MB)';
      }
      return { notification, projectPath: response.path, sizeWarning: response.size_warning };
    } catch (err) {
      logIpcError('document.saveProject', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const saveProjectAs = createAsyncThunk(
  'document/saveProjectAs',
  async (path: string, { rejectWithValue }) => {
    try {
      const response = await saveProjectAsIPC(path);
      let notification = `Project saved: ${response.path.split(/[/\\]/).pop() ?? response.path}`;
      if (response.size_warning) {
        notification += ' (large project — uncompressed layers exceed 256 MB)';
      }
      return { notification, projectPath: response.path, sizeWarning: response.size_warning };
    } catch (err) {
      logIpcError('document.saveProjectAs', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const exportPattern = createAsyncThunk(
  'document/exportPattern',
  async (
    args: { layerId: number; path: string; name?: string },
    { rejectWithValue }
  ) => {
    try {
      await exportPatternIPC(args);
      return 'Pattern exported';
    } catch (err) {
      logIpcError('document.exportPattern', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const importPattern = createAsyncThunk(
  'document/importPattern',
  async (
    args: { path: string; targetLayerId: number },
    { rejectWithValue }
  ) => {
    try {
      const response = await importPatternIPC(args.path, args.targetLayerId);
      return {
        notification: 'Pattern imported',
        filterIds: response.filter_ids,
        paletteIds: response.palette_ids,
      };
    } catch (err) {
      logIpcError('document.importPattern', err);
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
      action: PayloadAction<
        Partial<
          Pick<
            DocumentState,
            | 'docId'
            | 'width'
            | 'height'
            | 'hasDocument'
            | 'hydrated'
            | 'layerId'
            | 'error'
            | 'notification'
            | 'loading'
            | 'projectPath'
            | 'sourcePath'
            | 'dirty'
            | 'documentEpoch'
          >
        >
      >
    ) {
      Object.assign(state, action.payload);
    },
    setDirty(state, action: PayloadAction<boolean>) {
      state.dirty = action.payload;
    },
    bumpDocumentEpoch(state) {
      state.documentEpoch += 1;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(refreshDocument.fulfilled, (state, action) => {
        state.docId = action.payload.docId;
        state.width = action.payload.width;
        state.height = action.payload.height;
        state.hasDocument = action.payload.hasDocument;
        state.hydrated = true;
        state.error = null;
      })
      .addCase(refreshDocument.rejected, (state, action) => {
        state.docId = null;
        state.width = 0;
        state.height = 0;
        state.hasDocument = false;
        state.hydrated = true;
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
        state.hydrated = true;
        state.projectPath = null;
        state.sourcePath = action.meta.arg;
        state.dirty = false;
        state.documentEpoch += 1;
        state.error = null;
      })
      .addCase(openImage.rejected, (state, action) => {
        state.loading = false;
        state.error = (action.payload as string) ?? 'Failed to open image';
      })
      .addCase(createDocument.pending, (state) => {
        state.loading = true;
        state.error = null;
        state.notification = null;
      })
      .addCase(createDocument.fulfilled, (state, action) => {
        state.loading = false;
        state.docId = action.payload.docId;
        state.width = action.payload.width;
        state.height = action.payload.height;
        state.layerId = action.payload.layerId;
        state.hasDocument = true;
        state.hydrated = true;
        state.projectPath = null;
        state.sourcePath = null;
        state.dirty = false;
        state.documentEpoch += 1;
        state.error = null;
      })
      .addCase(createDocument.rejected, (state, action) => {
        state.loading = false;
        state.error = (action.payload as string) ?? 'Failed to create document';
      })
      .addCase(importImageLayer.pending, (state) => {
        state.loading = true;
        state.error = null;
        state.notification = null;
      })
      .addCase(importImageLayer.fulfilled, (state, action) => {
        state.loading = false;
        state.layerId = action.payload.layerId;
        state.error = null;
      })
      .addCase(importImageLayer.rejected, (state, action) => {
        state.loading = false;
        state.error = (action.payload as string) ?? 'Failed to import image layer';
      })
      .addCase(saveImage.fulfilled, (state, action) => {
        state.notification = action.payload;
        state.error = null;
      })
      .addCase(saveImage.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to save image';
        state.notification = null;
      })
      .addCase(openProject.pending, (state) => {
        state.loading = true;
        state.error = null;
        state.notification = null;
      })
      .addCase(openProject.fulfilled, (state, action) => {
        state.loading = false;
        state.docId = action.payload.docId;
        state.width = action.payload.width;
        state.height = action.payload.height;
        state.layerId = action.payload.layerId;
        state.hasDocument = true;
        state.hydrated = true;
        state.projectPath = action.payload.projectPath;
        state.sourcePath = null;
        state.dirty = false;
        state.documentEpoch += 1;
        state.error = null;
      })
      .addCase(openProject.rejected, (state, action) => {
        state.loading = false;
        state.error = (action.payload as string) ?? 'Failed to open project';
      })
      .addCase(saveProject.fulfilled, (state, action) => {
        state.notification = action.payload.notification;
        state.projectPath = action.payload.projectPath;
        state.dirty = false;
        state.error = null;
      })
      .addCase(saveProject.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to save project';
        state.notification = null;
      })
      .addCase(saveProjectAs.fulfilled, (state, action) => {
        state.notification = action.payload.notification;
        state.projectPath = action.payload.projectPath;
        state.dirty = false;
        state.error = null;
      })
      .addCase(saveProjectAs.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to save project';
        state.notification = null;
      })
      .addCase(exportPattern.fulfilled, (state, action) => {
        state.notification = action.payload;
        state.error = null;
      })
      .addCase(exportPattern.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to export pattern';
        state.notification = null;
      })
      .addCase(importPattern.fulfilled, (state, action) => {
        state.notification = action.payload.notification;
        state.error = null;
      })
      .addCase(importPattern.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to import pattern';
        state.notification = null;
      });
  },
});

export const { clearNotification, clearError, setDocumentMeta, setDirty, bumpDocumentEpoch } = documentSlice.actions;
export default documentSlice.reducer;
