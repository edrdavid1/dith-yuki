import { createSlice, createAsyncThunk, createSelector, type PayloadAction } from '@reduxjs/toolkit';
import type { FilterInfo } from '../../types';
import {
  formatIpcError,
  getDocumentSnapshot,
  logIpcError,
  removeFilter as removeFilterIPC,
  reorderFilter as reorderFilterIPC,
  updateFilter as updateFilterIPC,
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

export const toggleFilterEnabled = createAsyncThunk(
  'filters/toggleEnabled',
  async (
    args: { layerId: number; filterId: string },
    { getState, dispatch, rejectWithValue }
  ) => {
    const state = getState() as { filters: FiltersState };
    const filter = state.filters.byId[args.filterId];
    if (!filter) return;
    const record = filter.params as unknown as Record<string, unknown>;
    const { type: _type, ...params } = record;
    try {
      await updateFilterIPC(args.layerId, args.filterId, params, {
        enabled: !filter.enabled,
      });
      await dispatch(refreshFilters());
    } catch (err) {
      logIpcError('filters.toggleEnabled', err);
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
      action: PayloadAction<{
        id: string;
        opacity?: number;
        blend_mode?: string;
        enabled?: boolean;
      }>
    ) {
      const filter = state.byId[action.payload.id];
      if (!filter) return;
      if (typeof action.payload.opacity === 'number') {
        filter.opacity = action.payload.opacity;
      }
      if (typeof action.payload.blend_mode === 'string') {
        filter.blend_mode = action.payload.blend_mode;
      }
      if (typeof action.payload.enabled === 'boolean') {
        filter.enabled = action.payload.enabled;
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
      })
      .addCase(toggleFilterEnabled.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      });
  },
});

export const { clearFilters, setFiltersError, patchFilter } = filtersSlice.actions;

const EMPTY_FILTERS: FilterInfo[] = [];

export const selectFiltersList = createSelector(
  [
    (state: { filters: FiltersState }) => state.filters.orderOnImageSource,
    (state: { filters: FiltersState }) => state.filters.byId,
  ],
  (order, byId): FilterInfo[] => {
    if (order.length === 0) return EMPTY_FILTERS;
    return order.map((id) => byId[id]).filter(Boolean);
  },
);

export default filtersSlice.reducer;
