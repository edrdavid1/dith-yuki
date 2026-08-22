import { createAsyncThunk } from '@reduxjs/toolkit';
import { bumpVersion, publishPaletteBinding } from './slices/palettesSlice';
import { setColors, setError, setName, setSelectedPaletteId } from './slices/colorLabSlice';
import { getAutoExtractPalettesPref } from './shell/ShellContext';
import {
  formatIpcError,
  generatePalette,
  logIpcError,
  type PaletteDto,
} from '../shared/ipc';
import { createColorEntry } from '../features/color-lab/types';
import { toHex } from '../types/effects';
import type { AppDispatch, RootState } from './store';

export type ExtractPaletteArgs = {
  layerId: number;
  /** Defaults to colorLab.extractCount when omitted. */
  targetCount?: number;
  /** Defaults to colorLab.extractMethod when omitted. */
  method?: string;
};

/**
 * Same path as Color Lab Extract: generate_palette → fill draft → lastCreatedId.
 * Failures set colorLab error UI; callers should not treat rejection as fatal for open/import.
 */
export const extractPalette = createAsyncThunk<
  PaletteDto,
  ExtractPaletteArgs,
  { state: RootState; rejectValue: string }
>('colorLab/extractPalette', async (args, { dispatch, getState, rejectWithValue }) => {
  const { extractCount, extractMethod, chromaWeight, contrastWeight } = getState().colorLab;
  const targetCount = args.targetCount ?? extractCount;
  const method = args.method ?? extractMethod;

  const docId = getState().document.docId;
  if (docId == null) {
    return rejectWithValue('No document open');
  }

  dispatch(setError(null));
  try {
    const dto = await generatePalette(docId, args.layerId, targetCount, method, {
      chromaWeight,
      contrastWeight,
    });
    dispatch(setName(dto.name || 'Untitled Palette'));
    dispatch(setColors(dto.colors.map(([r, g, b]) => createColorEntry(toHex(r, g, b)))));
    dispatch(bumpVersion({ lastCreatedId: dto.id }));
    publishPaletteBinding(dto.id);
    dispatch(setSelectedPaletteId(dto.id));
    return dto;
  } catch (err: unknown) {
    const message = formatIpcError(err);
    dispatch(setError(message));
    logIpcError('extractPalette', err);
    return rejectWithValue(message);
  }
});

/**
 * After successful open/import of an image layer: run Extract when the shell pref is on.
 * Never throws — extract failures stay in Color Lab error UI and do not undo open/import.
 */
export async function maybeAutoExtractPalette(
  dispatch: AppDispatch,
  layerId: number,
  enabled: boolean = getAutoExtractPalettesPref()
): Promise<void> {
  if (!enabled) return;
  await dispatch(extractPalette({ layerId }));
}
