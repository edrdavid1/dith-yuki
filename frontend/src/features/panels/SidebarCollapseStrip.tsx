/**
 * Collapsed sidebar icon strip — shared by legacy DockedSidebar and FlexLayout columns.
 */

import type { MutableRefObject } from 'react';
import type { DockSide, PanelId } from '../../types/panels';
import { PANEL_DISPLAY_NAMES } from '../../types/panels';
import ResizeHandle from '../../components/common/ResizeHandle';
import styles from '../../app/AppLayout.module.css';
import resizeStyles from '../../shared/ui/ResizeHandle.module.css';
import { bind } from '../../shared/ui/cn';
import Icon from '../../icons/iconRegistry';

const cn = bind({ ...styles, ...resizeStyles });

export type SidebarCollapseStripProps = {
  side: DockSide;
  panelIds: PanelId[];
  onExpand: () => void;
  hitTargetRef?: MutableRefObject<HTMLElement | null>;
};

function panelIconName(panelId: PanelId): 'effect.dithering' | 'layers' | 'color-lab' {
  if (panelId === 'effect') return 'effect.dithering';
  if (panelId === 'layers') return 'layers';
  return 'color-lab';
}

export default function SidebarCollapseStrip({
  side,
  panelIds,
  onExpand,
  hitTargetRef,
}: SidebarCollapseStripProps) {
  const collapsedAreaClass =
    side === 'left' ? 'sidebar-collapsed-area-left' : 'sidebar-collapsed-area-right';

  const handleCollapsedResize = (delta: number) => {
    const expandDelta = side === 'right' ? delta < -10 : delta > 10;
    if (expandDelta) onExpand();
  };

  return (
    <div
      ref={(el) => {
        if (hitTargetRef) hitTargetRef.current = el;
      }}
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
          onClick={onExpand}
          title={PANEL_DISPLAY_NAMES[panelId] ?? panelId}
        >
          <span className={cn('sidebar-collapsed-btn-icon')}>
            <Icon name={panelIconName(panelId)} width={20} height={20} />
          </span>
        </button>
      ))}
    </div>
  );
}
