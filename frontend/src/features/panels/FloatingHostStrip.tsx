/**
 * W2: permanent return UI when a sidebar has only floating panels.
 * Click icon → focus popout; double-click or Return → dock that panel / all.
 */

import type { MutableRefObject } from 'react';
import type { DockSide, PanelId } from '../../types/panels';
import { PANEL_DISPLAY_NAMES } from '../../types/panels';
import styles from '../../app/AppLayout.module.css';
import { bind } from '../../shared/ui/cn';
import Icon from '../../icons/iconRegistry';
import { appEdgesAttr } from '../../shared/ui/appEdges';
import { isMacOS } from '../../lib/platform';

const cn = bind(styles);

export type FloatingHostStripProps = {
  side: DockSide;
  panelIds: PanelId[];
  onFocusPanel: (panelId: PanelId) => void;
  onReturnPanel: (panelId: PanelId) => void;
  onReturnAll: () => void;
  hitTargetRef?: MutableRefObject<HTMLElement | null>;
  affinityArmed?: boolean;
};

function panelIconName(panelId: PanelId): 'effect.dithering' | 'layers' | 'color-lab' {
  if (panelId === 'effect') return 'effect.dithering';
  if (panelId === 'layers') return 'layers';
  return 'color-lab';
}

export default function FloatingHostStrip({
  side,
  panelIds,
  onFocusPanel,
  onReturnPanel,
  onReturnAll,
  hitTargetRef,
  affinityArmed,
}: FloatingHostStripProps) {
  const collapsedAreaClass =
    side === 'left' ? 'sidebar-collapsed-area-left' : 'sidebar-collapsed-area-right';
  const edges = isMacOS()
    ? appEdgesAttr('bottom', side === 'left' && 'left', side === 'right' && 'right')
    : undefined;

  return (
    <div
      ref={(el) => {
        if (hitTargetRef) hitTargetRef.current = el;
      }}
      className={cn(
        'sidebar-collapsed',
        collapsedAreaClass,
        side === 'left' && 'sidebar-collapsed-left',
        affinityArmed &&
          (side === 'left'
            ? 'sidebar-empty-drop-edge-armed'
            : 'sidebar-empty-drop-edge-armed'),
      )}
      data-app-edges={edges}
      data-dock-empty-edge={side}
      data-floating-host-strip
    >
      <div className={cn('sidebar-collapsed-decor-panel')} />
      {panelIds.map((panelId) => (
        <button
          key={panelId}
          type="button"
          data-panel-id={panelId}
          className={cn('sidebar-collapsed-btn')}
          onClick={() => onFocusPanel(panelId)}
          onDoubleClick={(e) => {
            e.preventDefault();
            onReturnPanel(panelId);
          }}
          title={`${PANEL_DISPLAY_NAMES[panelId] ?? panelId} — click to focus, double-click to return`}
        >
          <span className={cn('sidebar-collapsed-btn-icon')}>
            <Icon name={panelIconName(panelId)} width={20} height={20} />
          </span>
        </button>
      ))}
      <button
        type="button"
        className={cn('sidebar-collapsed-btn')}
        onClick={onReturnAll}
        title="Return panels to dock"
        aria-label="Return panels to dock"
        data-floating-host-return
      >
        <span className={cn('sidebar-collapsed-btn-icon')} style={{ fontSize: 9, letterSpacing: 0 }}>
          Return
        </span>
      </button>
    </div>
  );
}
