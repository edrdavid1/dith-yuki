import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import {
  formatIpcError,
  getSelection,
  logIpcError,
  setSelection as setSelectionIPC,
} from '../../shared/ipc';

export interface SelectionState {
  layerId: number | null;
  filterId: string | null;
  error: string | null;
  /** When true, remote selection-changed events are ignored (local echo guard). */
  suppressRemote: boolean;
}

const initialState: SelectionState = {
  layerId: null,
  filterId: null,
  error: null,
  suppressRemote: false,
};

export const fetchSelection = createAsyncThunk(
  'selection/fetch',
  async (_, { rejectWithValue }) => {
    try {
      return await getSelection();
    } catch (err) {
      logIpcError('selection.fetch', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

const selectionSlice = createSlice({
  name: 'selection',
  initialState,
  reducers: {
    applyLocal(
      state,
      action: PayloadAction<{ layerId: number | null; filterId: string | null }>
    ) {
      state.suppressRemote = true;
      state.layerId = action.payload.layerId;
      state.filterId = action.payload.filterId;
      state.error = null;
    },
    applyRemote(
      state,
      action: PayloadAction<{ layerId: number | null; filterId: string | null }>
    ) {
      if (state.suppressRemote) return;
      state.layerId = action.payload.layerId;
      state.filterId = action.payload.filterId;
    },
    setSuppressRemote(state, action: PayloadAction<boolean>) {
      state.suppressRemote = action.payload;
    },
    setSelectionError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchSelection.fulfilled, (state, action) => {
        state.layerId = action.payload.selected_layer_id;
        state.filterId = action.payload.selected_filter_id;
        state.error = null;
      })
      .addCase(fetchSelection.rejected, (state, action) => {
        state.error = (action.payload as string) ?? 'Failed to fetch selection';
      });
  },
});

export const { applyLocal, applyRemote, setSuppressRemote, setSelectionError } =
  selectionSlice.actions;

export const setSelection = createAsyncThunk(
  'selection/set',
  async (
    args: { layerId: number | null; filterId: string | null },
    { dispatch, rejectWithValue }
  ) => {
    dispatch(applyLocal(args));
    try {
      await setSelectionIPC(args.layerId, args.filterId);
      setTimeout(() => {
        dispatch(setSuppressRemote(false));
      }, 0);
      return args;
    } catch (err) {
      logIpcError('selection.set', err);
      dispatch(setSuppressRemote(false));
      const message = formatIpcError(err);
      dispatch(setSelectionError(message));
      return rejectWithValue(message);
    }
  }
);

export default selectionSlice.reducer;
