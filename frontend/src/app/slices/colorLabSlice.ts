import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  createColorEntry,
  type ColorEntry,
  type ColorLabDraftSnapshot,
  type ExtractMethod,
} from '../../features/color-lab/types';

export type { ColorLabDraftSnapshot };

const STORAGE_KEY = 'dither.colorLab.draft';

export interface ColorLabState extends ColorLabDraftSnapshot {
  error: string | null;
  successMessage: string | null;
  /** Ignore remote draft events briefly after local publish (echo guard). */
  suppressRemote: boolean;
  /** Bumps when draft arrives from another window — local sync must not re-broadcast. */
  remoteEpoch: number;
  /** Shared swatch cursor: manual-edit list and Oklab volume viewer. */
  selectedColorIndex: number | null;
  /** Document palette Apply will replace; null → add. */
  selectedPaletteId: number | null;
  chromaWeight: number;
  contrastWeight: number;
}

const defaultDraft = (): ColorLabDraftSnapshot => ({
  name: 'Untitled Palette',
  colors: [],
  extractMethod: 'MedianCut',
  extractCount: 8,
  chromaWeight: 0,
  contrastWeight: 0,
});

function clampWeight(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

function storageAvailable(): boolean {
  try {
    return typeof localStorage !== 'undefined' && localStorage !== null;
  } catch {
    return false;
  }
}

function loadPersistedDraft(): ColorLabDraftSnapshot {
  if (!storageAvailable()) return defaultDraft();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaultDraft();
    const parsed = JSON.parse(raw) as Partial<ColorLabDraftSnapshot>;
    const colors = Array.isArray(parsed.colors)
      ? parsed.colors.map((c) => createColorEntry(typeof c?.hex === 'string' ? c.hex : '#000000'))
      : [];
    return {
      name: typeof parsed.name === 'string' && parsed.name.trim() ? parsed.name : 'Untitled Palette',
      colors,
      extractMethod: parsed.extractMethod === 'KMeans' ? 'KMeans' : 'MedianCut',
      extractCount:
        typeof parsed.extractCount === 'number'
          ? Math.min(64, Math.max(2, Math.round(parsed.extractCount)))
          : 8,
      chromaWeight: clampWeight(parsed.chromaWeight),
      contrastWeight: clampWeight(parsed.contrastWeight),
    };
  } catch {
    return defaultDraft();
  }
}

export function persistColorLabDraft(draft: ColorLabDraftSnapshot): void {
  if (!storageAvailable()) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(draft));
  } catch {
    // ignore quota / private mode
  }
}

export function selectDraftSnapshot(state: ColorLabState): ColorLabDraftSnapshot {
  return {
    name: state.name,
    colors: state.colors,
    extractMethod: state.extractMethod,
    extractCount: state.extractCount,
    chromaWeight: state.chromaWeight,
    contrastWeight: state.contrastWeight,
  };
}

const hydrated = loadPersistedDraft();

function clampSelectedIndex(length: number, index: number | null): number | null {
  if (index === null || index < 0 || index >= length) return null;
  return index;
}

const initialState: ColorLabState = {
  ...hydrated,
  error: null,
  successMessage: null,
  suppressRemote: false,
  remoteEpoch: 0,
  selectedColorIndex: null,
  selectedPaletteId: null,
  chromaWeight: hydrated.chromaWeight ?? 0,
  contrastWeight: hydrated.contrastWeight ?? 0,
};

const colorLabSlice = createSlice({
  name: 'colorLab',
  initialState,
  reducers: {
    setName(state, action: PayloadAction<string>) {
      state.name = action.payload;
      state.error = null;
    },
    setColors(state, action: PayloadAction<ColorEntry[]>) {
      state.colors = action.payload;
      state.selectedColorIndex = clampSelectedIndex(
        state.colors.length,
        state.selectedColorIndex
      );
      state.error = null;
    },
    setColorAt(state, action: PayloadAction<{ index: number; hex: string }>) {
      const { index, hex } = action.payload;
      if (index < 0 || index >= state.colors.length) return;
      state.colors[index] = createColorEntry(hex);
      state.error = null;
    },
    addColor(state, action: PayloadAction<string | undefined>) {
      if (state.colors.length >= 256) return;
      state.colors.push(createColorEntry(action.payload ?? '#000000'));
      state.error = null;
    },
    deleteColor(state, action: PayloadAction<number>) {
      const removed = action.payload;
      state.colors = state.colors.filter((_, i) => i !== removed);
      if (state.selectedColorIndex === removed) {
        state.selectedColorIndex = null;
      } else if (state.selectedColorIndex !== null && state.selectedColorIndex > removed) {
        state.selectedColorIndex -= 1;
      }
      state.error = null;
    },
    setSelectedColorIndex(state, action: PayloadAction<number | null>) {
      state.selectedColorIndex = clampSelectedIndex(state.colors.length, action.payload);
    },
    setExtractMethod(state, action: PayloadAction<ExtractMethod>) {
      state.extractMethod = action.payload;
    },
    setExtractCount(state, action: PayloadAction<number>) {
      state.extractCount = Math.min(64, Math.max(2, Math.round(action.payload)));
    },
    setChromaWeight(state, action: PayloadAction<number>) {
      state.chromaWeight = clampWeight(action.payload);
    },
    setContrastWeight(state, action: PayloadAction<number>) {
      state.contrastWeight = clampWeight(action.payload);
    },
    setSelectedPaletteId(state, action: PayloadAction<number | null>) {
      state.selectedPaletteId = action.payload;
    },
    resetDraft(state) {
      const next = defaultDraft();
      state.name = next.name;
      state.colors = next.colors;
      state.extractMethod = next.extractMethod;
      state.extractCount = next.extractCount;
      state.chromaWeight = next.chromaWeight ?? 0;
      state.contrastWeight = next.contrastWeight ?? 0;
      state.selectedColorIndex = null;
      state.selectedPaletteId = null;
      state.error = null;
      state.successMessage = null;
    },
    setError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
    setSuccessMessage(state, action: PayloadAction<string | null>) {
      state.successMessage = action.payload;
    },
    applyRemoteDraft(state, action: PayloadAction<ColorLabDraftSnapshot>) {
      if (state.suppressRemote) return;
      state.name = action.payload.name;
      state.colors = action.payload.colors;
      state.extractMethod = action.payload.extractMethod;
      state.extractCount = action.payload.extractCount;
      state.chromaWeight = clampWeight(action.payload.chromaWeight);
      state.contrastWeight = clampWeight(action.payload.contrastWeight);
      state.selectedColorIndex = clampSelectedIndex(
        state.colors.length,
        state.selectedColorIndex
      );
      state.remoteEpoch += 1;
    },
    setSuppressRemote(state, action: PayloadAction<boolean>) {
      state.suppressRemote = action.payload;
    },
    hydrateFromStorage(state) {
      const draft = loadPersistedDraft();
      state.name = draft.name;
      state.colors = draft.colors;
      state.extractMethod = draft.extractMethod;
      state.extractCount = draft.extractCount;
      state.chromaWeight = draft.chromaWeight ?? 0;
      state.contrastWeight = draft.contrastWeight ?? 0;
      state.selectedColorIndex = clampSelectedIndex(
        state.colors.length,
        state.selectedColorIndex
      );
    },
  },
});

export const {
  setName,
  setColors,
  setColorAt,
  addColor,
  deleteColor,
  setSelectedColorIndex,
  setExtractMethod,
  setExtractCount,
  setChromaWeight,
  setContrastWeight,
  setSelectedPaletteId,
  resetDraft,
  setError,
  setSuccessMessage,
  applyRemoteDraft,
  setSuppressRemote,
  hydrateFromStorage,
} = colorLabSlice.actions;

export default colorLabSlice.reducer;
