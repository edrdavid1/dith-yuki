import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import { emitPaletteBindingChanged } from '../../shared/ipc/events';

export interface PalettesState {
  version: number;
  lastCreatedId: number | null;
  error: string | null;
}

const LAST_CREATED_KEY = 'dither.palettes.lastCreatedId';

function storageAvailable(): boolean {
  try {
    return typeof localStorage !== 'undefined' && localStorage !== null;
  } catch {
    return false;
  }
}

export function loadPersistedLastCreatedId(): number | null {
  if (!storageAvailable()) return null;
  try {
    const raw = localStorage.getItem(LAST_CREATED_KEY);
    if (raw == null || raw === '') return null;
    const id = Number(raw);
    return Number.isFinite(id) ? id : null;
  } catch {
    return null;
  }
}

export function persistLastCreatedId(id: number | null): void {
  if (!storageAvailable()) return;
  try {
    if (id == null) localStorage.removeItem(LAST_CREATED_KEY);
    else localStorage.setItem(LAST_CREATED_KEY, String(id));
  } catch {
    // ignore quota / private mode
  }
}

export function publishPaletteBinding(id: number | null): void {
  persistLastCreatedId(id);
  void emitPaletteBindingChanged(id);
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
    applyRemoteBinding(state, action: PayloadAction<number | null>) {
      state.lastCreatedId = action.payload;
      state.version += 1;
    },
    setPalettesError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
    clearLastCreatedId(state) {
      state.lastCreatedId = null;
    },
  },
});

export const { bumpVersion, applyRemoteBinding, setPalettesError, clearLastCreatedId } =
  palettesSlice.actions;
export default palettesSlice.reducer;
