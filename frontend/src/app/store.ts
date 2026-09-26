import { configureStore } from '@reduxjs/toolkit';
import documentReducer from './slices/documentSlice';
import tabsReducer from './slices/tabsSlice';
import layersReducer from './slices/layersSlice';
import filtersReducer from './slices/filtersSlice';
import selectionReducer from './slices/selectionSlice';
import palettesReducer from './slices/palettesSlice';
import colorLabReducer from './slices/colorLabSlice';
import undoReducer from './slices/undoSlice';

const isDev =
  typeof import.meta !== 'undefined' &&
  Boolean((import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV);

export function createAppStore() {
  return configureStore({
    reducer: {
      document: documentReducer,
      tabs: tabsReducer,
      layers: layersReducer,
      filters: filtersReducer,
      selection: selectionReducer,
      palettes: palettesReducer,
      colorLab: colorLabReducer,
      undo: undoReducer,
    },
    devTools: isDev,
    middleware: (getDefaultMiddleware) => {
      const middleware = getDefaultMiddleware({
        serializableCheck: false,
      });
      return middleware;
    },
  });
}

export type AppStore = ReturnType<typeof createAppStore>;
export type RootState = ReturnType<AppStore['getState']>;
export type AppDispatch = AppStore['dispatch'];

/** Singleton store for the current window. */
export const store = createAppStore();
