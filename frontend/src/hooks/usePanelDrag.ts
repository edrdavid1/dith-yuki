import { useState, useCallback, useRef, useEffect } from 'react';
import { computeInsertIndex } from '../features/panels/panelDockGeometry';
import {
  resolvePanelDragMode,
  type PanelDragMode,
} from '../features/panels/panelDragMode';
import type { DockSide, PanelId } from '../types/panels';

// =============================================================================
// Types
// =============================================================================

export interface DragState {
  active: boolean;
  panelId: PanelId | null;
  mode: 'idle' | PanelDragMode;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  dropIndex: number | null;
  sourceIndex: number;
}

export interface UsePanelDragOptions {
  sidebarRef: React.RefObject<HTMLDivElement | null>;
  /** Side this drag stack belongs to. */
  side: DockSide;
  /** Opposite column for cross-sidebar moves (expanded or collapsed hit target). */
  oppositeHitRef?: React.RefObject<HTMLElement | null>;
  oppositeSide: DockSide;
  panelOrder: PanelId[];
  onReorder: (newOrder: PanelId[]) => void;
  onUndock: (panelId: PanelId, width: number, height: number, screenX: number, screenY: number) => void;
  /** Move docked panel to the opposite side without floating. */
  onCrossMove?: (panelId: PanelId, side: DockSide, insertIndex: number) => void;
}

export interface UsePanelDragReturn {
  dragState: DragState;
  handleMouseDown: (panelId: PanelId, event: React.MouseEvent) => void;
  getPanelStyle: (panelId: PanelId) => React.CSSProperties;
  dropIndicatorIndex: number | null;
}

// =============================================================================
// Constants
// =============================================================================

const DRAG_THRESHOLD_PX = 5;

const INITIAL_DRAG_STATE: DragState = {
  active: false,
  panelId: null,
  mode: 'idle',
  startX: 0,
  startY: 0,
  currentX: 0,
  currentY: 0,
  dropIndex: null,
  sourceIndex: -1,
};

// =============================================================================
// Hook
// =============================================================================

export function usePanelDrag(options: UsePanelDragOptions): UsePanelDragReturn {
  const { panelOrder, sidebarRef, side, oppositeSide } = options;

  const [dragState, setDragState] = useState<DragState>(INITIAL_DRAG_STATE);

  const sidebarRefLatest = useRef(sidebarRef);
  sidebarRefLatest.current = sidebarRef;
  const oppositeHitRefLatest = useRef(options.oppositeHitRef);
  oppositeHitRefLatest.current = options.oppositeHitRef;
  const sideRef = useRef(side);
  sideRef.current = side;
  const oppositeSideRef = useRef(oppositeSide);
  oppositeSideRef.current = oppositeSide;
  const onReorderRef = useRef(options.onReorder);
  onReorderRef.current = options.onReorder;
  const onUndockRef = useRef(options.onUndock);
  onUndockRef.current = options.onUndock;
  const onCrossMoveRef = useRef(options.onCrossMove);
  onCrossMoveRef.current = options.onCrossMove;
  const panelOrderRef = useRef(panelOrder);
  panelOrderRef.current = panelOrder;
  const dragStateRef = useRef(dragState);
  dragStateRef.current = dragState;

  const pendingRef = useRef<{
    panelId: PanelId;
    startX: number;
    startY: number;
    sourceIndex: number;
  } | null>(null);

  const activeRef = useRef(false);

  const handlersRef = useRef<{
    onMouseMove: (e: MouseEvent) => void;
    onMouseUp: (e: MouseEvent) => void;
    onKeyDown: (e: KeyboardEvent) => void;
  } | null>(null);

  if (!handlersRef.current) {
    const computeMode = (clientX: number, panelId?: string): PanelDragMode => {
      const own = sidebarRefLatest.current?.current?.getBoundingClientRect() ?? null;
      const oppEl = oppositeHitRefLatest.current?.current;
      const opp = oppEl?.getBoundingClientRect() ?? null;
      return resolvePanelDragMode({
        clientX,
        side: sideRef.current,
        panelId: panelId ?? pendingRef.current?.panelId,
        ownRect: own,
        oppositeRect: opp,
        oppositeSide: oppositeSideRef.current,
        viewportWidth: window.innerWidth,
      });
    };

    const computeDropIndexIn = (
      container: HTMLElement | null,
      clientY: number,
      sourcePanelId: string | null
    ): number | null => {
      if (!container) return null;
      const panelElements = container.querySelectorAll<HTMLElement>('[data-panel-id]');
      const midpoints: number[] = [];
      panelElements.forEach((el) => {
        if (el.getAttribute('data-panel-id') === sourcePanelId) return;
        const rect = el.getBoundingClientRect();
        if (rect.height <= 0) return;
        midpoints.push(rect.top + rect.height / 2);
      });
      if (panelElements.length === 0) return 0;
      if (midpoints.length === 0) return 0;
      return computeInsertIndex(midpoints, clientY);
    };

    const computeDropIndex = (clientY: number): number | null => {
      return computeDropIndexIn(
        sidebarRefLatest.current?.current ?? null,
        clientY,
        pendingRef.current?.panelId ?? null
      );
    };

    const cleanup = () => {
      document.removeEventListener('mousemove', handlersRef.current!.onMouseMove);
      document.removeEventListener('mouseup', handlersRef.current!.onMouseUp);
      document.removeEventListener('keydown', handlersRef.current!.onKeyDown);
      document.body.style.userSelect = '';
      delete document.body.dataset.panelDockDrag;
      delete document.body.dataset.dockDropSide;
      activeRef.current = false;
      pendingRef.current = null;
      setDragState(INITIAL_DRAG_STATE);
    };

    const syncDropCue = (mode: PanelDragMode) => {
      document.body.dataset.panelDockDrag = '1';
      if (mode === 'cross') {
        document.body.dataset.dockDropSide = oppositeSideRef.current;
      } else {
        delete document.body.dataset.dockDropSide;
      }
    };

    handlersRef.current = {
      onMouseMove: (e: MouseEvent) => {
        const pending = pendingRef.current;

        if (!activeRef.current && pending) {
          const dx = e.clientX - pending.startX;
          const dy = e.clientY - pending.startY;
          if (Math.sqrt(dx * dx + dy * dy) >= DRAG_THRESHOLD_PX) {
            activeRef.current = true;
            document.body.style.userSelect = 'none';
            const mode = computeMode(e.clientX);
            syncDropCue(mode);
            setDragState({
              active: true,
              panelId: pending.panelId,
              mode,
              startX: pending.startX,
              startY: pending.startY,
              currentX: e.clientX,
              currentY: e.clientY,
              dropIndex: null,
              sourceIndex: pending.sourceIndex,
            });
          }
          return;
        }

        if (activeRef.current) {
          const mode = computeMode(e.clientX);
          syncDropCue(mode);
          const dropIndex =
            mode === 'cross'
              ? computeDropIndexIn(
                  oppositeHitRefLatest.current?.current ?? null,
                  e.clientY,
                  pendingRef.current?.panelId ?? null
                )
              : computeDropIndex(e.clientY);
          setDragState((prev) => ({
            ...prev,
            currentX: e.clientX,
            currentY: e.clientY,
            mode,
            dropIndex,
          }));
        }
      },

      onMouseUp: (e: MouseEvent) => {
        if (!activeRef.current) {
          cleanup();
          return;
        }

        const state = dragStateRef.current;
        const panelId = state.panelId;
        const currentMode = computeMode(e.clientX, panelId ?? undefined);

        if (currentMode === 'cross' && panelId && onCrossMoveRef.current) {
          const insertIndex =
            computeDropIndexIn(
              oppositeHitRefLatest.current?.current ?? null,
              e.clientY,
              panelId
            ) ?? 0;
          onCrossMoveRef.current(panelId, oppositeSideRef.current, insertIndex);
        } else if (currentMode === 'undock' && panelId) {
          const sidebar = sidebarRefLatest.current?.current;
          const panelEl =
            panelId === 'preview'
              ? document.querySelector<HTMLElement>(`[data-panel-id="preview"]`)
              : sidebar?.querySelector<HTMLElement>(`[data-panel-id="${panelId}"]`);
          if (panelEl) {
            const rect = panelEl.getBoundingClientRect();
            onUndockRef.current(panelId, rect.width, rect.height, e.screenX, e.screenY);
          }
        } else if (currentMode === 'reorder' && panelId && state.dropIndex != null) {
          const currentOrder = [...panelOrderRef.current];
          const sourceIndex = state.sourceIndex;
          const dropIndex = state.dropIndex;
          const [removed] = currentOrder.splice(sourceIndex, 1);
          const insertAt = dropIndex > sourceIndex ? dropIndex - 1 : dropIndex;
          currentOrder.splice(insertAt, 0, removed);
          onReorderRef.current(currentOrder);
        }

        cleanup();
      },

      onKeyDown: (e: KeyboardEvent) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          cleanup();
        }
      },
    };
  }

  const handleMouseDown = useCallback((panelId: PanelId, event: React.MouseEvent) => {
    if (event.button !== 0) return;
    event.preventDefault();

    const sourceIndex = panelOrderRef.current.indexOf(panelId);

    pendingRef.current = {
      panelId,
      startX: event.clientX,
      startY: event.clientY,
      sourceIndex: sourceIndex >= 0 ? sourceIndex : -1,
    };

    document.addEventListener('mousemove', handlersRef.current!.onMouseMove);
    document.addEventListener('mouseup', handlersRef.current!.onMouseUp);
    document.addEventListener('keydown', handlersRef.current!.onKeyDown);
  }, []);

  useEffect(() => {
    return () => {
      if (handlersRef.current) {
        document.removeEventListener('mousemove', handlersRef.current.onMouseMove);
        document.removeEventListener('mouseup', handlersRef.current.onMouseUp);
        document.removeEventListener('keydown', handlersRef.current.onKeyDown);
      }
      document.body.style.userSelect = '';
      delete document.body.dataset.panelDockDrag;
      delete document.body.dataset.dockDropSide;
    };
  }, []);

  const getPanelStyle = useCallback(
    (panelId: PanelId): React.CSSProperties => {
      if (dragState.active && dragState.panelId === panelId) {
        return { opacity: 0.4, pointerEvents: 'none' };
      }
      return {};
    },
    [dragState.active, dragState.panelId]
  );

  return {
    dragState,
    handleMouseDown,
    getPanelStyle,
    dropIndicatorIndex:
      dragState.active && dragState.mode === 'reorder' ? dragState.dropIndex : null,
  };
}
