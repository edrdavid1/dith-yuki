import type React from 'react';
import type { DockSide } from '../../types/panels';

/** Chrome-only props passed from layout hosts into feature panels. */
export type PanelChromeProps = {
  onTitleBarMouseDown?: (e: React.MouseEvent) => void;
  /** Current dock side when docked in a sidebar (enables Move to other side). */
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
};
