import type React from 'react';
import type { DockSide, PanelId } from '../../types/panels';
import LayersFeature from '../layers/LayersFeature';
import EffectsFeature from '../effects/EffectsFeature';
import ColorLabFeature from '../color-lab/ColorLabFeature';

type DockedPanelContentProps = {
  panelId: PanelId;
  onTitleBarMouseDown: (e: React.MouseEvent) => void;
  dockSide: DockSide;
  onMoveToSide: (side: DockSide) => void;
};

/** Maps panelId → connected feature; layout only passes chrome. */
export default function DockedPanelContent({
  panelId,
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
}: DockedPanelContentProps) {
  const chrome = { onTitleBarMouseDown, dockSide, onMoveToSide };
  switch (panelId) {
    case 'effect':
      return <EffectsFeature {...chrome} />;
    case 'layers':
      return <LayersFeature {...chrome} />;
    case 'colorlab':
      return (
        <ColorLabFeature
          variant="sidebar"
          showTitlebar
          {...chrome}
        />
      );
    default:
      return null;
  }
}
