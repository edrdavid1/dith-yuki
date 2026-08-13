import { useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { redo, undo } from '../app/slices/undoSlice';

/**
 * Window-level ⌘Z / Ctrl+Z (and shift variants). When `canUndo` is true,
 * steals the chord from focused inputs so NumberInput does not consume it.
 */
export function useUndoShortcuts() {
  const dispatch = useAppDispatch();
  const hasDocument = useAppSelector((s) => s.document.hasDocument);
  const canUndo = useAppSelector((s) => s.undo.canUndo);
  const canRedo = useAppSelector((s) => s.undo.canRedo);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!hasDocument) return;
      const mod = e.metaKey || e.ctrlKey;
      if (!mod || e.key.toLowerCase() !== 'z') return;

      if (e.shiftKey) {
        if (!canRedo) return;
        e.preventDefault();
        e.stopPropagation();
        void dispatch(redo());
        return;
      }

      if (!canUndo) return;
      e.preventDefault();
      e.stopPropagation();
      void dispatch(undo());
    };

    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [dispatch, hasDocument, canUndo, canRedo]);
}
