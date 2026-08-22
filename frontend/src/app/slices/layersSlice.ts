import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import type { LayerNodeDto, LayerPropsPatch } from '../../shared/types/layers';
import {
  addFilter,
  addLayer as addLayerIPC,
  formatIpcError,
  getDocumentSnapshot,
  getLayerTree,
  logIpcError,
  removeLayer as removeLayerIPC,
  reorderLayer as reorderLayerIPC,
  setLayerProps as setLayerPropsIPC,
} from '../../shared/ipc';
import type { SnapshotLayerNode } from '../../shared/ipc/document';
import type { EffectType } from '../../types/effects';
import { EFFECT_DEFAULTS, EFFECT_TO_FILTER_KIND, validateDocumentStructure } from '../../types/effects';
import type { RootState } from '../store';

export type LayersStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface LayersState {
  tree: LayerNodeDto[];
  status: LayersStatus;
  error: string | null;
}

const initialState: LayersState = {
  tree: [],
  status: 'idle',
  error: null,
};

function extractLayerId(id: { inner: number } | number | undefined): number | null {
  if (id === undefined || id === null) return null;
  if (typeof id === 'number') return id;
  if (typeof id === 'object' && 'inner' in id) return id.inner;
  return null;
}

function flattenSnapshotLayers(
  nodes: SnapshotLayerNode[]
): { id: number; filters: unknown[] }[] {
  const result: { id: number; filters: unknown[] }[] = [];
  for (const node of nodes) {
    const id = extractLayerId(node.id as { inner: number } | number | undefined);
    if (id !== null) {
      result.push({ id, filters: node.filters ?? [] });
    }
    if (node.children) {
      result.push(...flattenSnapshotLayers(node.children));
    }
  }
  return result;
}

function findLayerById(layers: LayerNodeDto[], layerId: number): LayerNodeDto | null {
  for (const layer of layers) {
    if (layer.id === layerId) return layer;
    if (layer.children) {
      const found = findLayerById(layer.children, layerId);
      if (found) return found;
    }
  }
  return null;
}

function findLayerIndex(layers: LayerNodeDto[], layerId: number | null): number | null {
  if (layerId === null) return null;
  const idx = layers.findIndex((l) => l.id === layerId);
  return idx >= 0 ? idx : null;
}

export const refreshLayers = createAsyncThunk(
  'layers/refresh',
  async (docId: number | null, { rejectWithValue }) => {
    if (docId === null) {
      return { tree: [] as LayerNodeDto[], validationError: null as string | null };
    }
    try {
      const tree = await getLayerTree();
      try {
        const snapshotResponse = await getDocumentSnapshot();
        const flatLayers = flattenSnapshotLayers(snapshotResponse.snapshot.layers);
        const validation = validateDocumentStructure(flatLayers);
        if (!validation.valid) {
          return {
            tree,
            validationError: `Invalid document structure: layer ${validation.layerId} has incorrect filter count`,
          };
        }
      } catch (err) {
        logIpcError('layers.refresh.getDocumentSnapshot(validation)', err);
      }
      return { tree, validationError: null };
    } catch (err) {
      logIpcError('layers.refresh', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const addRasterLayer = createAsyncThunk(
  'layers/addRaster',
  async (
    args: { docId: number | null; selectedLayerId: number | null; layers: LayerNodeDto[] },
    { dispatch, rejectWithValue }
  ) => {
    if (args.docId === null) return;
    try {
      const index = findLayerIndex(args.layers, args.selectedLayerId);
      await addLayerIPC(args.docId, 'raster', null, index !== null ? index + 1 : args.layers.length);
      await dispatch(refreshLayers(args.docId));
    } catch (err) {
      logIpcError('layers.addRaster', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const removeLayer = createAsyncThunk(
  'layers/remove',
  async (args: { docId: number | null; layerId: number }, { dispatch, rejectWithValue }) => {
    if (args.docId === null) return;
    try {
      await removeLayerIPC(args.docId, args.layerId);
      await dispatch(refreshLayers(args.docId));
    } catch (err) {
      logIpcError('layers.remove', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const addLayerWithEffect = createAsyncThunk(
  'layers/addWithEffect',
  async (
    args: { docId: number | null; layers: LayerNodeDto[]; effectType: EffectType },
    { dispatch, getState, rejectWithValue }
  ) => {
    if (args.docId === null) return null;
    try {
      const imageSourceLayer = args.layers.length > 0 ? args.layers[0] : null;
      if (!imageSourceLayer) {
        return rejectWithValue('No image source layer found');
      }
      const filterKind = EFFECT_TO_FILTER_KIND[args.effectType];
      const defaultParams = { ...EFFECT_DEFAULTS[args.effectType] };
      // New palette-based filters default to lastCreatedId when params have empty/None palette_id
      const lastCreatedId = (getState() as RootState).palettes.lastCreatedId;
      if (
        lastCreatedId != null &&
        'palette_id' in defaultParams &&
        (defaultParams.palette_id === null || defaultParams.palette_id === undefined)
      ) {
        defaultParams.palette_id = lastCreatedId;
      }
      await addFilter(args.docId, imageSourceLayer.id, filterKind, defaultParams);
      await dispatch(refreshLayers(args.docId));
      return imageSourceLayer.id;
    } catch (err) {
      logIpcError('layers.addWithEffect', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const toggleLayerVisibility = createAsyncThunk(
  'layers/toggleVisibility',
  async (
    args: { docId: number | null; layerId: number; layers: LayerNodeDto[] },
    { dispatch, rejectWithValue }
  ) => {
    if (args.docId === null) return;
    const layer = findLayerById(args.layers, args.layerId);
    if (!layer) return;
    try {
      await setLayerPropsIPC(args.docId, args.layerId, { visible: !layer.visible });
      await dispatch(refreshLayers(args.docId));
    } catch (err) {
      logIpcError('layers.toggleVisibility', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const reorderLayer = createAsyncThunk(
  'layers/reorder',
  async (
    args: { docId: number | null; layerId: number; newParent: number | null; newIndex: number },
    { dispatch, rejectWithValue }
  ) => {
    if (args.docId === null) return;
    try {
      await reorderLayerIPC(args.docId, args.layerId, args.newParent, args.newIndex);
      await dispatch(refreshLayers(args.docId));
    } catch (err) {
      logIpcError('layers.reorder', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const patchLayerProps = createAsyncThunk(
  'layers/patchProps',
  async (
    args: { docId: number | null; layerId: number; patch: LayerPropsPatch },
    { dispatch, rejectWithValue }
  ) => {
    if (args.docId === null) return;
    try {
      await setLayerPropsIPC(args.docId, args.layerId, args.patch);
      await dispatch(refreshLayers(args.docId));
    } catch (err) {
      logIpcError('layers.patchProps', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

const layersSlice = createSlice({
  name: 'layers',
  initialState,
  reducers: {
    clearLayers(state) {
      state.tree = [];
      state.status = 'idle';
      state.error = null;
    },
    setLayersError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(refreshLayers.pending, (state) => {
        state.status = 'loading';
      })
      .addCase(refreshLayers.fulfilled, (state, action) => {
        state.tree = action.payload.tree;
        state.status = 'ready';
        state.error = action.payload.validationError;
      })
      .addCase(refreshLayers.rejected, (state, action) => {
        state.status = 'error';
        state.error = (action.payload as string) ?? 'Failed to refresh layers';
      })
      .addCase(addRasterLayer.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(removeLayer.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(addLayerWithEffect.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(toggleLayerVisibility.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(reorderLayer.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(patchLayerProps.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      });
  },
});

export const { clearLayers, setLayersError } = layersSlice.actions;
export default layersSlice.reducer;
