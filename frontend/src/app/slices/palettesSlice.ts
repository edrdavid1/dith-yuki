import { createSlice, type PayloadAction } from '@reduxjs/toolkit';

export interface PalettesState {
  version: number;
  lastCreatedId: number | null;
  error: string | null;
}

const initialState: PalettesState = {
  version: 0,
  lastCreatedId: null,
  error: null,
};

const palettesSlice = createSlice({
  name: 'palettes',
  initialState,
  reducers: {
    bumpVersion(state, action: PayloadAction<{ lastCreatedId?: number | null } | undefined>) {
      state.version += 1;
      if (action.payload && 'lastCreatedId' in action.payload) {
        state.lastCreatedId = action.payload.lastCreatedId ?? null;
      }
      state.error = null;
    },
    setPalettesError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
    clearLastCreatedId(state) {
      state.lastCreatedId = null;
    },
  },
});

export const { bumpVersion, setPalettesError, clearLastCreatedId } = palettesSlice.actions;
export default palettesSlice.reducer;
