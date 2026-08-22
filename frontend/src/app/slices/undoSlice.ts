import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  formatIpcError,
  logIpcError,
  redo as redoIPC,
  undo as undoIPC,
  type UndoStateDto,
} from '../../shared/ipc';

export interface UndoState {
  canUndo: boolean;
  canRedo: boolean;
}

const initialState: UndoState = {
  canUndo: false,
  canRedo: false,
};

export const undo = createAsyncThunk('undo/undo', async (docId: number, { rejectWithValue }) => {
  try {
    return await undoIPC(docId);
  } catch (err) {
    logIpcError('undo', err);
    return rejectWithValue(formatIpcError(err));
  }
});

export const redo = createAsyncThunk('undo/redo', async (docId: number, { rejectWithValue }) => {
  try {
    return await redoIPC(docId);
  } catch (err) {
    logIpcError('redo', err);
    return rejectWithValue(formatIpcError(err));
  }
});

function applyDto(state: UndoState, dto: UndoStateDto) {
  state.canUndo = dto.can_undo;
  state.canRedo = dto.can_redo;
}

const undoSlice = createSlice({
  name: 'undo',
  initialState,
  reducers: {
    applyUndoState(state, action: PayloadAction<UndoStateDto>) {
      applyDto(state, action.payload);
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(undo.fulfilled, (state, action) => {
        applyDto(state, action.payload);
      })
      .addCase(redo.fulfilled, (state, action) => {
        applyDto(state, action.payload);
      });
  },
});

export const { applyUndoState } = undoSlice.actions;
export default undoSlice.reducer;
