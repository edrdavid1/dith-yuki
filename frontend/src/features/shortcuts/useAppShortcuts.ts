import { useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { redo, undo } from '../../app/slices/undoSlice';
import { addLayerWithEffect, refreshLayers } from '../../app/slices/layersSlice';
import { removeFilter as removeFilterThunk, selectFiltersList } from '../../app/slices/filtersSlice';
import { setSelection } from '../../app/slices/selectionSlice';
import type { EffectType } from '../../types/effects';
import { findMatchingShortcut, isEditableKeyboardTarget } from './bindings';
import { getDocumentCommands, getLayoutCommands, getPreviewCommands } from './commandRegistry';
import { useShortcuts } from './ShortcutsContext';

function filterKindToEffect(kind: string): EffectType | null {
  switch (kind) {
    case 'DitherV2':
    case 'Dither':
      return 'Dithering';
    case 'Glitch':
      return 'Glitching';
    case 'Curves':
      return 'Curves';
    case 'Levels':
      return 'RGBChannels';
    case 'Glow':
      return 'Glow';
    case 'Crt':
      return 'CRT';
    case 'Adjust':
      return 'Adjust';
    default:
      return null;
  }
}

/**
 * Photoshop-style window shortcuts (defaults + user bindings from Preferences).
 */
export function useAppShortcuts() {
  const dispatch = useAppDispatch();
  const { bindings, capturing } = useShortcuts();
  const hasDocument = useAppSelector((s) => s.document.hasDocument);
  const canUndo = useAppSelector((s) => s.undo.canUndo);
  const canRedo = useAppSelector((s) => s.undo.canRedo);
  const docId = useAppSelector((s) => s.document.docId);
  const layers = useAppSelector((s) => s.layers.tree);
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const selectedFilterId = useAppSelector((s) => s.selection.filterId);
  const filters = useAppSelector(selectFiltersList);
  const imageSourceId = layers[0]?.id ?? selectedLayerId;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (capturing) return;
      const id = findMatchingShortcut(e, bindings);
      if (!id) return;

      const editable = isEditableKeyboardTarget(e.target);
      if (editable && id !== 'undo' && id !== 'redo') return;

      const docs = getDocumentCommands();
      const preview = getPreviewCommands();
      const layout = getLayoutCommands();
      const steal = () => {
        e.preventDefault();
        e.stopPropagation();
      };

      switch (id) {
        case 'newProject':
          steal();
          docs?.newProject();
          return;
        case 'openImage':
          steal();
          docs?.openImage();
          return;
        case 'openProject':
          steal();
          docs?.openProject();
          return;
        case 'saveProject':
          if (!hasDocument) return;
          steal();
          docs?.saveProject();
          return;
        case 'saveProjectAs':
          if (!hasDocument) return;
          steal();
          docs?.saveProjectAs();
          return;
        case 'undo':
          if (!hasDocument || !canUndo) return;
          steal();
          void dispatch(undo());
          return;
        case 'redo':
          if (!hasDocument || !canRedo) return;
          steal();
          void dispatch(redo());
          return;
        case 'newLayer':
          if (!hasDocument) return;
          steal();
          void dispatch(setSelection({ layerId: null, filterId: null }));
          return;
        case 'duplicateLayer': {
          if (!hasDocument) return;
          const selected = filters.find((f) => f.id === selectedFilterId);
          const effectType = selected ? filterKindToEffect(selected.kind) : 'Dithering';
          if (!effectType) return;
          steal();
          void dispatch(addLayerWithEffect({ docId, layers, effectType })).then((result) => {
            if (addLayerWithEffect.fulfilled.match(result) && result.payload != null) {
              void dispatch(setSelection({ layerId: result.payload, filterId: null }));
            }
          });
          return;
        }
        case 'deleteLayer':
          if (!hasDocument || !selectedFilterId || imageSourceId == null) return;
          steal();
          void dispatch(
            removeFilterThunk({ layerId: imageSourceId, filterId: selectedFilterId })
          ).then(() => {
            void dispatch(setSelection({ layerId: imageSourceId, filterId: null }));
            void dispatch(refreshLayers(docId));
          });
          return;
        case 'zoomIn':
          if (!hasDocument) return;
          steal();
          preview?.zoomIn();
          return;
        case 'zoomOut':
          if (!hasDocument) return;
          steal();
          preview?.zoomOut();
          return;
        case 'zoomFit':
          if (!hasDocument) return;
          steal();
          preview?.fitToView();
          return;
        case 'zoomActual':
          if (!hasDocument) return;
          steal();
          preview?.actualPixels();
          return;
        case 'preferences':
          steal();
          docs?.openPreferences();
          return;
        case 'focusMode':
          steal();
          layout?.toggleFocusMode();
          return;
      }
    };

    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [
    bindings,
    capturing,
    canRedo,
    canUndo,
    dispatch,
    docId,
    filters,
    hasDocument,
    imageSourceId,
    layers,
    selectedFilterId,
  ]);
}
