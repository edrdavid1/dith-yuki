import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import type { FilterInfo } from '../../types';
import {
  formatIpcError,
  getDocumentSnapshot,
  logIpcError,
  removeFilter as removeFilterIPC,
  reorderFilter as reorderFilterIPC,
} from '../../shared/ipc';
import { unwrapFilterParams } from '../../shared/unwrapFilterParams';

export type FiltersStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface FiltersState {
  byId: Record<string, FilterInfo>;
  orderOnImageSource: string[];
  status: FiltersStatus;
  error: string | null;
}

const initialState: FiltersState = {
  byId: {},
  orderOnImageSource: [],
  status: 'idle',
  error: null,
};

function unwrapParams(params: Record<string, unknown>): Record<string, unknown> {
  return unwrapFilterParams(params);
}

export const refreshFilters = createAsyncThunk(
  'filters/refresh',
  async (_, { rejectWithValue }) => {
    try {
      const response = await getDocumentSnapshot();
      const imageLayer = response.snapshot.layers[0];
      if (!imageLayer?.filters) {
        return [] as FilterInfo[];
      }
      return imageLayer.filters.map((f) => ({
        id: typeof f.id === 'string' ? f.id : String(f.id),
        kind: f.kind as FilterInfo['kind'],
        params: unwrapParams(f.params),
        enabled: f.enabled ?? true,
        opacity: typeof f.opacity === 'number' ? f.opacity : 1,
        blend_mode: typeof f.blend_mode === 'string' ? f.blend_mode : 'Normal',
      })) as unknown as FilterInfo[];
    } catch (err) {
      logIpcError('filters.refresh', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const removeFilter = createAsyncThunk(
  'filters/remove',
  async (
    args: { layerId: number; filterId: string },
    { dispatch, rejectWithValue }
  ) => {
    try {
      await removeFilterIPC(args.layerId, args.filterId);
      await dispatch(refreshFilters());
      return args.filterId;
    } catch (err) {
      logIpcError('filters.remove', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const reorderFilter = createAsyncThunk(
  'filters/reorder',
  async (
    args: { layerId: number; filterId: string; newIndex: number },
    { dispatch, rejectWithValue }
  ) => {
    try {
      await reorderFilterIPC(args.layerId, args.filterId, args.newIndex);
      await dispatch(refreshFilters());
    } catch (err) {
      logIpcError('filters.reorder', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

const filtersSlice = createSlice({
  name: 'filters',
  initialState,
  reducers: {
    clearFilters(state) {
      state.byId = {};
      state.orderOnImageSource = [];
      state.status = 'idle';
      state.error = null;
    },
    setFiltersError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
    patchFilter(
      state,
      action: PayloadAction<{ id: string; opacity?: number; blend_mode?: string }>
    ) {
      const filter = state.byId[action.payload.id];
      if (!filter) return;
      if (typeof action.payload.opacity === 'number') {
        filter.opacity = action.payload.opacity;
      }
      if (typeof action.payload.blend_mode === 'string') {
        filter.blend_mode = action.payload.blend_mode;
      }
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(refreshFilters.pending, (state) => {
        state.status = 'loading';
      })
      .addCase(refreshFilters.fulfilled, (state, action) => {
        state.byId = {};
        state.orderOnImageSource = [];
        for (const filter of action.payload) {
          state.byId[filter.id] = filter;
          state.orderOnImageSource.push(filter.id);
        }
        state.status = 'ready';
        state.error = null;
      })
      .addCase(refreshFilters.rejected, (state, action) => {
        state.status = 'error';
        state.error = (action.payload as string) ?? 'Failed to refresh filters';
      })
      .addCase(removeFilter.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(reorderFilter.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      });
  },
});

export const { clearFilters, setFiltersError, patchFilter } = filtersSlice.actions;

export function selectFiltersList(state: { filters: FiltersState }): FilterInfo[] {
  return state.filters.orderOnImageSource
    .map((id) => state.filters.byId[id])
    .filter(Boolean);
}

export default filtersSlice.reducer;
