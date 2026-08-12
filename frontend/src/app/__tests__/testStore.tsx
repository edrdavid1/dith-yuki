import React, { type ReactNode } from 'react';
import { Provider } from 'react-redux';
import { configureStore } from '@reduxjs/toolkit';
import documentReducer from '../slices/documentSlice';
import layersReducer from '../slices/layersSlice';
import filtersReducer from '../slices/filtersSlice';
import selectionReducer from '../slices/selectionSlice';
import panelsReducer from '../slices/panelsSlice';
import palettesReducer from '../slices/palettesSlice';
import colorLabReducer from '../slices/colorLabSlice';
import type { RootState } from '../store';

/** Fresh store for unit tests (no engine bridge). */
export function createTestStore(preloadedState?: Partial<RootState>) {
  return configureStore({
    reducer: {
      document: documentReducer,
      layers: layersReducer,
      filters: filtersReducer,
      selection: selectionReducer,
      panels: panelsReducer,
      palettes: palettesReducer,
      colorLab: colorLabReducer,
    },
    preloadedState: preloadedState as RootState | undefined,
    middleware: (getDefaultMiddleware) =>
      getDefaultMiddleware({ serializableCheck: false }),
  });
}

export function StoreProvider({
  children,
  store,
}: {
  children: ReactNode;
  store?: ReturnType<typeof createTestStore>;
}) {
  const s = store ?? createTestStore();
  return <Provider store={s}>{children}</Provider>;
}
