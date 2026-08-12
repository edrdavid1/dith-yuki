import { useCallback, useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { refreshDocument } from '../app/slices/documentSlice';

export interface DocumentState {
  docId: number | null;
  width: number;
  height: number;
  hasDocument: boolean;
}

/**
 * Thin adapter over RTK `document` slice for floating panels.
 * Engine bridge keeps the slice fresh; this also triggers an initial refresh.
 */
export function useDocumentState(): DocumentState & { error: string | null } {
  const dispatch = useAppDispatch();
  const docId = useAppSelector((s) => s.document.docId);
  const width = useAppSelector((s) => s.document.width);
  const height = useAppSelector((s) => s.document.height);
  const hasDocument = useAppSelector((s) => s.document.hasDocument);
  const error = useAppSelector((s) => s.document.error);

  useEffect(() => {
    void dispatch(refreshDocument());
  }, [dispatch]);

  return { docId, width, height, hasDocument, error };
}
