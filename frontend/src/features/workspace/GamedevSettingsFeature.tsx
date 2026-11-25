/**
 * Connected Gamedev Settings — Tile + Sprites release slice.
 */

import type { PanelChromeProps } from '../panels/PanelChrome';
import GamedevSettingsPanel from './GamedevSettingsPanel';

export default function GamedevSettingsFeature({
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
  hideChrome,
  onPopOut,
  onDockBack,
}: PanelChromeProps) {
  return (
    <GamedevSettingsPanel
      onTitleBarMouseDown={onTitleBarMouseDown}
      dockSide={dockSide}
      onMoveToSide={onMoveToSide}
      hideChrome={hideChrome}
      onPopOut={onPopOut}
      onDockBack={onDockBack}
    />
  );
}
