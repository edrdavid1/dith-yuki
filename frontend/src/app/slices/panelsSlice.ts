import { createSlice, createAsyncThunk, type PayloadAction } from '@reduxjs/toolkit';
import type { DockSide, PanelId, PanelInfo, PanelStateSnapshot } from '../../types/panels';
import { FLOATING_ONLY_PANELS } from '../../types/panels';
import {
  dockPanel,
  formatIpcError,
  getPanelsState,
  hidePanel,
  logIpcError,
  showPanel,
  undockPanel,
} from '../../shared/ipc';

export interface PanelsState {
  entities: PanelInfo[];
  leftOrder: PanelId[];
  rightOrder: PanelId[];
  error: string | null;
}

const initialState: PanelsState = {
  entities: [],
  leftOrder: [],
  rightOrder: [],
  error: null,
};

export function applySnapshot(state: PanelsState, snapshot: PanelStateSnapshot) {
  state.entities = snapshot.panels;
  state.leftOrder = snapshot.left_order;
  state.rightOrder = snapshot.right_order;
}

/** Docked+visible panel IDs on one side, in side order (excludes floating-only). */
export function selectVisibleDocked(
  entities: PanelInfo[],
  leftOrder: PanelId[],
  rightOrder: PanelId[],
  side: DockSide
): PanelId[] {
  const order = side === 'left' ? leftOrder : rightOrder;
  return order.filter((id) => {
    if (FLOATING_ONLY_PANELS.has(id)) return false;
    const panel = entities.find((p) => p.id === id);
    return Boolean(panel?.docked && panel?.visible);
  });
}

export const fetchPanels = createAsyncThunk(
  'panels/fetch',
  async (_, { rejectWithValue }) => {
    try {
      return await getPanelsState();
    } catch (err) {
      logIpcError('panels.fetch', err);
      return rejectWithValue(formatIpcError(err));
    }
  }
);

export const undock = createAsyncThunk(
  'panels/undock',
  async (panelId: string, { rejectWithValue }) => {
    try {
      await undockPanel(panelId);
    } catch (err) {
      logIpcError('panels.undock', err);
      return rejectWithValue(`undock failed: ${formatIpcError(err)}`);
    }
  }
);

export const dock = createAsyncThunk(
  'panels/dock',
  async (panelId: string, { rejectWithValue }) => {
    try {
      await dockPanel(panelId);
    } catch (err) {
      logIpcError('panels.dock', err);
      return rejectWithValue(`dock failed: ${formatIpcError(err)}`);
    }
  }
);

export const hide = createAsyncThunk(
  'panels/hide',
  async (panelId: string, { rejectWithValue }) => {
    try {
      await hidePanel(panelId);
    } catch (err) {
      logIpcError('panels.hide', err);
      return rejectWithValue(`hide failed: ${formatIpcError(err)}`);
    }
  }
);

export const show = createAsyncThunk(
  'panels/show',
  async (panelId: string, { rejectWithValue }) => {
    try {
      await showPanel(panelId);
    } catch (err) {
      logIpcError('panels.show', err);
      return rejectWithValue(`show failed: ${formatIpcError(err)}`);
    }
  }
);

export function isPanelSnapshot(payload: unknown): payload is PanelStateSnapshot {
  return (
    typeof payload === 'object' &&
    payload !== null &&
    Array.isArray((payload as PanelStateSnapshot).panels) &&
    Array.isArray((payload as PanelStateSnapshot).left_order) &&
    Array.isArray((payload as PanelStateSnapshot).right_order)
  );
}

const panelsSlice = createSlice({
  name: 'panels',
  initialState,
  reducers: {
    applyPanelEvent(state, action: PayloadAction<PanelStateSnapshot | PanelInfo[]>) {
      const payload = action.payload;
      if (Array.isArray(payload)) {
        // Legacy panels-only payload: update entities, leave orders intact.
        state.entities = payload;
      } else if (isPanelSnapshot(payload)) {
        applySnapshot(state, payload);
      }
    },
    setPanelsError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchPanels.fulfilled, (state, action) => {
        applySnapshot(state, action.payload);
        state.error = null;
      })
      .addCase(fetchPanels.rejected, (state, action) => {
        state.error = `getPanelsState failed: ${(action.payload as string) ?? 'unknown'}`;
      })
      .addCase(undock.fulfilled, (state) => {
        state.error = null;
      })
      .addCase(undock.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(dock.fulfilled, (state) => {
        state.error = null;
      })
      .addCase(dock.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(hide.fulfilled, (state) => {
        state.error = null;
      })
      .addCase(hide.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      })
      .addCase(show.fulfilled, (state) => {
        state.error = null;
      })
      .addCase(show.rejected, (state, action) => {
        state.error = (action.payload as string) ?? state.error;
      });
  },
});

export const { applyPanelEvent, setPanelsError } = panelsSlice.actions;
export default panelsSlice.reducer;
