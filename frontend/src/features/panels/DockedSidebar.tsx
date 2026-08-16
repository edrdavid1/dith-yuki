import React, { useCallback, useLayoutEffect, useRef } from 'react';
import ResizeHandle from '../../components/common/ResizeHandle';
import { usePanelDrag } from '../../hooks/usePanelDrag';
import { useDockZoneReporter } from '../../hooks/useDockZoneReporter';
import type { DockAffinityEvent } from '../../shared/ipc';
import { movePanelToSide, reorderSidebar, undockPanelWithSize } from '../../shared/ipc';
import type { DockSide, PanelId } from '../../types/panels';
import { PANEL_DISPLAY_NAMES } from '../../types/panels';
import { panelStackFlex } from './panelDragMode';
import DockedPanelContent from './DockedPanelContent';
import styles from '../../app/AppLayout.module.css';
import resizeStyles from '../../shared/ui/ResizeHandle.module.css';
import { bind } from '../../shared/ui/cn';
import Icon from '../../icons/iconRegistry';

const cn = bind({ ...styles, ...resizeStyles });

export type DockedSidebarProps = {
  side: DockSide;
  panelIds: PanelId[];
  width: number;
  collapsed: boolean;
  splitRatio: number;
  affinity: DockAffinityEvent | null;
  oppositeHitRef: React.RefObject<HTMLElement | null>;
  hitTargetRef: React.MutableRefObject<HTMLElement | null>;
  onCollapsedChange: (collapsed: boolean) => void;
  onWidthChange: (width: number | ((prev: number) => number)) => void;
  onSplitRatioChange: (ratio: number | ((prev: number) => number)) => void;
};

/** Compute effective column width for grid template. */
export function sidebarEffectiveWidth(
  panelCount: number,
  collapsed: boolean,
  width: number
): number {
  if (panelCount === 0) return 0;
  return collapsed ? 40 : width;
}

/**
 * Side-agnostic docked sidebar: collapsed strip + expanded stack + affinity drop cue.
 * Parent owns the CSS grid column width; this component fills its grid area.
 */
export default function DockedSidebar({
  side,
  panelIds,
  width,
  collapsed,
  splitRatio,
  affinity,
  oppositeHitRef,
  hitTargetRef,
  onCollapsedChange,
  onWidthChange,
  onSplitRatioChange,
}: DockedSidebarProps) {
  const sidebarRef = useRef<HTMLDivElement>(null);
  const collapsedRef = useRef<HTMLDivElement>(null);
  const emptyEdgeRef = useRef<HTMLDivElement>(null);
  const hasVisible = panelIds.length > 0;
  const oppositeSide: DockSide = side === 'left' ? 'right' : 'left';

  const syncHitTarget = useCallback(() => {
    if (!hasVisible && emptyEdgeRef.current) {
      hitTargetRef.current = emptyEdgeRef.current;
    } else if (collapsed && collapsedRef.current) {
      hitTargetRef.current = collapsedRef.current;
    } else if (hasVisible && !collapsed && sidebarRef.current) {
      hitTargetRef.current = sidebarRef.current;
    } else {
      hitTargetRef.current = null;
    }
  }, [collapsed, hasVisible, hitTargetRef]);

  // Keep parent hit ref current for cross-sidebar drag.
  useLayoutEffect(() => {
    syncHitTarget();
  }, [syncHitTarget, hasVisible, collapsed, panelIds]);

  const handleDragUndock = useCallback(
    async (panelId: PanelId, w: number, h: number, screenX: number, screenY: number) => {
      try {
        await undockPanelWithSize(
          panelId,
          Math.round(w),
          Math.round(h),
          Math.round(screenX),
          Math.round(screenY)
        );
      } catch (err) {
        console.error('Drag undock failed:', err);
      }
    },
    []
  );

  const handleDragReorder = useCallback(
    async (newOrder: PanelId[]) => {
      try {
        await reorderSidebar(side, newOrder);
      } catch (err) {
        console.error('Panel reorder failed:', err);
      }
    },
    [side]
  );

  const handleCrossMove = useCallback(
    async (panelId: PanelId, targetSide: DockSide, insertIndex: number) => {
      try {
        await movePanelToSide(panelId, targetSide, insertIndex);
      } catch (err) {
        console.error('Cross-sidebar move failed:', err);
      }
    },
    []
  );

  const { dragState, handleMouseDown, getPanelStyle, dropIndicatorIndex } = usePanelDrag({
    sidebarRef,
    side,
    oppositeSide,
    oppositeHitRef,
    panelOrder: panelIds,
    onReorder: handleDragReorder,
    onUndock: handleDragUndock,
    onCrossMove: handleCrossMove,
  });

  useDockZoneReporter({
    sidebarRef,
    collapsedRef,
    emptyEdgeRef,
    sidebarSide: side,
    sidebarCollapsed: collapsed,
    sidebarWidth: width,
    hasDockTargets: hasVisible,
    reportEmptyEdge: true,
  });

  const affinityInsertIndex =
    affinity?.armed && affinity.side === side && affinity.insertIndex != null
      ? affinity.insertIndex
      : null;

  const emptyEdgeArmed = Boolean(affinity?.armed && affinity.side === side);

  const showDropIndicator = (index: number) =>
    (dragState.active && dragState.mode === 'reorder' && dropIndicatorIndex === index) ||
    affinityInsertIndex === index;

  const handleSidebarResize = useCallback(
    (delta: number) => {
      onWidthChange((w) => {
        const signedDelta = side === 'right' ? -delta : delta;
        const newW = w + signedDelta;
        if (newW < 220) {
          requestAnimationFrame(() => onCollapsedChange(true));
          return w;
        }
        return Math.min(600, Math.max(240, newW));
      });
    },
    [side, onWidthChange, onCollapsedChange]
  );

  const handleCollapsedResize = useCallback(
    (delta: number) => {
      const expandDelta = side === 'right' ? delta < -10 : delta > 10;
      if (expandDelta) onCollapsedChange(false);
    },
    [side, onCollapsedChange]
  );

  const handleSplit = useCallback(
    (delta: number) => {
      const height = sidebarRef.current?.clientHeight ?? 0;
      if (height <= 0 || panelIds.length !== 2) return;
      onSplitRatioChange((prev) => prev + delta / height);
    },
    [onSplitRatioChange, panelIds.length]
  );

  const areaClass = side === 'left' ? 'sidebar-area-left' : 'sidebar-area-right';
  const collapsedAreaClass =
    side === 'left' ? 'sidebar-collapsed-area-left' : 'sidebar-collapsed-area-right';
  const resizeAreaClass =
    side === 'left' ? 'sidebar-resize-left' : 'sidebar-resize-right';

  return (
    <>
      {/* Empty side: canvas-edge drop rail so cross-sidebar drag has a real hit target. */}
      {!hasVisible && (
        <div
          ref={emptyEdgeRef}
          className={cn(
            'sidebar-empty-drop-edge',
            side === 'left' ? 'sidebar-empty-drop-edge-left' : 'sidebar-empty-drop-edge-right',
            emptyEdgeArmed && 'sidebar-empty-drop-edge-armed'
          )}
          data-dock-empty-edge={side}
          aria-hidden
        />
      )}

      {hasVisible && !collapsed && (
        <ResizeHandle
          direction="horizontal"
          onResize={handleSidebarResize}
          className={cn(
            'sidebar-resize-handle',
            resizeAreaClass,
            side === 'left' && 'sidebar-resize-handle-left'
          )}
        />
      )}

      {hasVisible && collapsed && (
        <div
          ref={collapsedRef}
          className={cn(
            'sidebar-collapsed',
            collapsedAreaClass,
            side === 'left' && 'sidebar-collapsed-left'
          )}
        >
          <ResizeHandle
            direction="horizontal"
            onResize={handleCollapsedResize}
            className={cn('sidebar-collapsed-resize')}
          />
          <div className={cn('sidebar-collapsed-decor-panel')} />
          {panelIds.map((panelId) => (
            <button
              key={panelId}
              type="button"
              data-panel-id={panelId}
              className={cn('sidebar-collapsed-btn')}
              onClick={() => onCollapsedChange(false)}
              title={PANEL_DISPLAY_NAMES[panelId] ?? panelId}
            >
              <span className={cn('sidebar-collapsed-btn-icon')}>
                <Icon
                  name={
                    panelId === 'effect'
                      ? 'effect.dithering'
                      : panelId === 'layers'
                        ? 'layers'
                        : 'color-lab'
                  }
                  width={20}
                  height={20}
                />
              </span>
            </button>
          ))}
        </div>
      )}

      <div
        className={cn('app-sidebar', areaClass)}
        ref={sidebarRef}
        style={{
          display: !hasVisible || collapsed ? 'none' : undefined,
        }}
      >
        {panelIds.map((panelId, index) => (
          <React.Fragment key={panelId}>
            {showDropIndicator(index) && <div className={cn('panel-drop-indicator')} />}
            <div
              data-panel-id={panelId}
              style={{
                flex: panelStackFlex(index, panelIds.length, splitRatio),
                display: 'flex',
                flexDirection: 'column',
                overflow: 'hidden',
                minHeight: 0,
                ...getPanelStyle(panelId),
              }}
            >
              <div style={{ flex: 1, overflow: 'hidden', minHeight: 0 }}>
                <DockedPanelContent
                  panelId={panelId}
                  dockSide={side}
                  onTitleBarMouseDown={(e) => handleMouseDown(panelId, e)}
                  onMoveToSide={(target) => {
                    void movePanelToSide(panelId, target).catch((err) =>
                      console.error('Move panel to side failed:', err)
                    );
                  }}
                />
              </div>
              {index < panelIds.length - 1 && (
                <ResizeHandle direction="vertical" onResize={handleSplit} />
              )}
            </div>
            {index === panelIds.length - 1 && showDropIndicator(panelIds.length) && (
              <div className={cn('panel-drop-indicator')} />
            )}
          </React.Fragment>
        ))}
      </div>
    </>
  );
}
