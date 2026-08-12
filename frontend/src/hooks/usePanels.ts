import { useCallback, useEffect, useMemo } from 'react';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import {
  dock as dockThunk,
  fetchPanels,
  hide as hideThunk,
  selectVisibleDocked,
  show as showThunk,
  undock as undockThunk,
} from '../app/slices/panelsSlice';
import type { DockSide, PanelId, PanelInfo } from '../types/panels';

export interface UsePanelsReturn {
  panels: PanelInfo[];
  leftOrder: PanelId[];
  rightOrder: PanelId[];
  /** Docked+visible IDs on a side, in side order. */
  visibleDocked: (side: DockSide) => PanelId[];
  undock: (panelId: string) => Promise<void>;
  dock: (panelId: string) => Promise<void>;
  hide: (panelId: string) => Promise<void>;
  show: (panelId: string) => Promise<void>;
  error: string | null;
}

/**
 * Thin adapter over RTK `panels` slice. Engine events update the store via listeners.
 */
export function usePanels(): UsePanelsReturn {
  const dispatch = useAppDispatch();
  const panels = useAppSelector((s) => s.panels.entities);
  const leftOrder = useAppSelector((s) => s.panels.leftOrder);
  const rightOrder = useAppSelector((s) => s.panels.rightOrder);
  const error = useAppSelector((s) => s.panels.error);

  useEffect(() => {
    void dispatch(fetchPanels());
  }, [dispatch]);

  const visibleDocked = useCallback(
    (side: DockSide) => selectVisibleDocked(panels, leftOrder, rightOrder, side),
    [panels, leftOrder, rightOrder]
  );

  // Stable memo for common dual reads (avoids recreating arrays in layout each render).
  const visibleLeft = useMemo(
    () => selectVisibleDocked(panels, leftOrder, rightOrder, 'left'),
    [panels, leftOrder, rightOrder]
  );
  const visibleRight = useMemo(
    () => selectVisibleDocked(panels, leftOrder, rightOrder, 'right'),
    [panels, leftOrder, rightOrder]
  );

  const undock = useCallback(async (panelId: string) => {
    await dispatch(undockThunk(panelId));
  }, [dispatch]);

  const dock = useCallback(async (panelId: string) => {
    await dispatch(dockThunk(panelId));
  }, [dispatch]);

  const hide = useCallback(async (panelId: string) => {
    await dispatch(hideThunk(panelId));
  }, [dispatch]);

  const show = useCallback(async (panelId: string) => {
    await dispatch(showThunk(panelId));
  }, [dispatch]);

  return {
    panels,
    leftOrder,
    rightOrder,
    visibleDocked: (side) => (side === 'left' ? visibleLeft : visibleRight),
    undock,
    dock,
    hide,
    show,
    error,
  };
}
