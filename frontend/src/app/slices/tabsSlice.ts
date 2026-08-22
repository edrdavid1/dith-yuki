import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  closeDocument,
  listOpenDocuments,
  setActiveDocument,
  type OpenDocumentTab,
  type OpenDocumentsPayload,
} from '../../shared/ipc/document';
import { logIpcError } from '../../shared/ipc';
import { refreshDocument } from './documentSlice';
import { refreshLayers } from './layersSlice';
import { refreshFilters } from './filtersSlice';

export interface TabsState {
  tabs: OpenDocumentTab[];
  activeId: number | null;
}

const initialState: TabsState = {
  tabs: [],
  activeId: null,
};

export const refreshTabs = createAsyncThunk('tabs/refresh', async () => {
  return listOpenDocuments();
});

export const activateTab = createAsyncThunk(
  'tabs/activate',
  async (docId: number, { dispatch }) => {
    await setActiveDocument(docId);
    const tabs = await listOpenDocuments();
    // Await document identity first — layers/filters must not refresh under the old docId.
    await dispatch(refreshDocument());
    void dispatch(refreshLayers(docId));
    void dispatch(refreshFilters());
    return tabs;
  }
);

export const closeTab = createAsyncThunk(
  'tabs/close',
  async (docId: number, { dispatch }) => {
    const tabs = await closeDocument(docId);
    await dispatch(refreshDocument());
    const nextId = tabs.active_id;
    void dispatch(refreshLayers(nextId));
    void dispatch(refreshFilters());
    return tabs;
  }
);

function applyPayload(state: TabsState, payload: OpenDocumentsPayload) {
  state.tabs = payload.tabs;
  state.activeId = payload.active_id;
}

const tabsSlice = createSlice({
  name: 'tabs',
  initialState,
  reducers: {
    tabsChanged(state, action: PayloadAction<OpenDocumentsPayload>) {
      applyPayload(state, action.payload);
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(refreshTabs.fulfilled, (state, action) => {
        applyPayload(state, action.payload);
      })
      .addCase(activateTab.fulfilled, (state, action) => {
        applyPayload(state, action.payload);
      })
      .addCase(closeTab.fulfilled, (state, action) => {
        applyPayload(state, action.payload);
      })
      .addCase(refreshTabs.rejected, (_state, action) => {
        logIpcError('tabs.refresh', action.error);
      });
  },
});

export const { tabsChanged } = tabsSlice.actions;
export default tabsSlice.reducer;
