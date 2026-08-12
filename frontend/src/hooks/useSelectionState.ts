import { useCallback, useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { fetchSelection, setSelection as setSelectionThunk } from '../app/slices/selectionSlice';

export interface SelectionState {
  selectedLayerId: number | null;
  selectedFilterId: string | null;
}

/**
 * Thin adapter over RTK `selection` slice (canonical cross-window selection).
 */
export function useSelectionState(): SelectionState & {
  setSelection: (layerId: number | null, filterId: string | null) => void;
  error: string | null;
} {
  const dispatch = useAppDispatch();
  const selectedLayerId = useAppSelector((s) => s.selection.layerId);
  const selectedFilterId = useAppSelector((s) => s.selection.filterId);
  const error = useAppSelector((s) => s.selection.error);

  useEffect(() => {
    void dispatch(fetchSelection());
  }, [dispatch]);

  const setSelection = useCallback(
    (layerId: number | null, filterId: string | null) => {
      void dispatch(setSelectionThunk({ layerId, filterId }));
    },
    [dispatch]
  );

  return { selectedLayerId, selectedFilterId, setSelection, error };
}
