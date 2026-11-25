/**
 * Gamedev Settings dockable panel — Color Lab shell (sidebar + scroll).
 */

import SimpleBar from 'simplebar-react';
import WindowTitlebar from '../../shared/ui/WindowTitlebar';
import type { PanelChromeProps } from '../panels/PanelChrome';
import { GamedevStrip } from './GamedevStrip';
import colorLabStyles from '../color-lab/ColorLabWindow.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(colorLabStyles);

export type GamedevSettingsPanelProps = PanelChromeProps;

export default function GamedevSettingsPanel({
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
  hideChrome,
  onPopOut,
  onDockBack,
}: GamedevSettingsPanelProps) {
  return (
    <div
      className={cn('color-lab-sidebar')}
      data-dock-window="gamedev"
      data-dock-side={dockSide}
    >
      {!hideChrome && (
        <WindowTitlebar
          title="Gamedev Settings"
          onMouseDown={onTitleBarMouseDown}
          dockSide={dockSide}
          onMoveToSide={onMoveToSide}
          onPopOut={onPopOut}
          onDockBack={onDockBack}
        />
      )}
      <div className={cn('color-lab-scroll')}>
        <SimpleBar style={{ height: '100%' }}>
          <GamedevStrip />
        </SimpleBar>
      </div>
    </div>
  );
}
