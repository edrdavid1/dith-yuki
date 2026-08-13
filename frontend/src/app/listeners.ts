import type { AppDispatch, AppStore } from './store';
import { refreshDocument } from './slices/documentSlice';
import { refreshLayers } from './slices/layersSlice';
import { refreshFilters } from './slices/filtersSlice';
import { applyRemote, fetchSelection } from './slices/selectionSlice';
import { applyPanelEvent, fetchPanels } from './slices/panelsSlice';
import { applyRemoteDraft, hydrateFromStorage } from './slices/colorLabSlice';
import { applyUndoState } from './slices/undoSlice';
import { bumpVersion } from './slices/palettesSlice';
import {
  onColorLabDraftChanged,
  onDocumentChanged,
  onPanelStateChanged,
  onSelectionChanged,
  onUndoStateChanged,
} from '../shared/ipc';
import type { PanelInfo, PanelStateSnapshot } from '../types/panels';

export type EngineBridgeCleanup = () => void;

/**
 * Subscribe once per window to Tauri engine events and mirror into the RTK store.
 * Call from Providers on mount; returns cleanup that unlistens all subscriptions.
 */
export function startEngineEventBridge(store: AppStore): EngineBridgeCleanup {
  const { dispatch } = store;
  const unsubscribers: Array<() => void> = [];
  let cancelled = false;

  // Initial hydrations
  void dispatch(fetchPanels());
  void dispatch(fetchSelection());
  dispatch(hydrateFromStorage());
  void dispatch(refreshDocument()).then(() => {
    const docId = store.getState().document.docId;
    if (docId !== null) {
      void dispatch(refreshLayers(docId));
      void dispatch(refreshFilters());
    }
  });

  onDocumentChanged((event) => {
    if (cancelled) return;
    const { kind } = event.payload;
    const docId = store.getState().document.docId;

    if (
      kind === 'layer_changed' ||
      kind === 'layer_reordered' ||
      kind === 'layer_added' ||
      kind === 'layer_removed' ||
      kind === 'filter_updated' ||
      kind === 'filter_added' ||
      kind === 'filter_removed' ||
      kind === 'filter_reordered' ||
      kind === 'document_undone' ||
      kind === 'document_redone'
    ) {
      if (docId !== null) {
        void dispatch(refreshLayers(docId));
      }
    }

    if (
      kind === 'filter_updated' ||
      kind === 'filter_added' ||
      kind === 'filter_removed' ||
      kind === 'filter_reordered' ||
      kind === 'document_undone' ||
      kind === 'document_redone'
    ) {
      void dispatch(refreshFilters());
    }

    if (kind === 'document_undone' || kind === 'document_redone') {
      dispatch(bumpVersion());
    }

    // Document open / structural changes — refresh meta
    void dispatch(refreshDocument());
  }).then((fn) => {
    if (cancelled) fn();
    else unsubscribers.push(fn);
  });

  onUndoStateChanged((event) => {
    if (cancelled) return;
    dispatch(applyUndoState(event.payload));
  }).then((fn) => {
    if (cancelled) fn();
    else unsubscribers.push(fn);
  });

  onSelectionChanged((event) => {
    if (cancelled) return;
    dispatch(
      applyRemote({
        layerId: event.payload.selected_layer_id,
        filterId: event.payload.selected_filter_id,
      })
    );
  }).then((fn) => {
    if (cancelled) fn();
    else unsubscribers.push(fn);
  });

  onPanelStateChanged((event) => {
    if (cancelled) return;
    dispatch(applyPanelEvent(event.payload as PanelStateSnapshot | PanelInfo[]));
  }).then((fn) => {
    if (cancelled) fn();
    else unsubscribers.push(fn);
  });

  onColorLabDraftChanged((event) => {
    if (cancelled) return;
    dispatch(applyRemoteDraft(event.payload));
  }).then((fn) => {
    if (cancelled) fn();
    else unsubscribers.push(fn);
  });

  return () => {
    cancelled = true;
    for (const un of unsubscribers) un();
  };
}

/** Convenience for tests that only have dispatch. */
export function startEngineEventBridgeWithDispatch(dispatch: AppDispatch, getState: AppStore['getState']): EngineBridgeCleanup {
  return startEngineEventBridge({ dispatch, getState } as AppStore);
}
