import { useAppSelector } from '../../app/hooks';
import { useShell } from '../../app/shell/ShellContext';
import { previewBackgroundStyle } from './previewBackground';
import PreviewFeature from '../preview/PreviewFeature';
import type { PanelChromeProps } from '../panels/PanelChrome';
import type { WelcomeActions } from '../../hooks/useWelcomeScreen';
import styles from './PreviewWindow.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind(styles);

/**
 * Main-window preview slot: respects docked/undocked preview panel state.
 */
export default function PreviewSlot({
  onTitleBarMouseDown,
  welcome,
}: PanelChromeProps & { welcome?: WelcomeActions }) {
  const panels = useAppSelector((s) => s.panels.entities);
  const previewPanel = panels.find((p) => p.id === 'preview');
  const previewDocked = !previewPanel || previewPanel.docked;
  const { previewBackground } = useShell();

  if (!previewDocked) {
    return (
      <div className={cn("preview-undocked-placeholder")} style={previewBackgroundStyle(previewBackground)}>
        <span>Preview is in a separate window</span>
      </div>
    );
  }

  return <PreviewFeature onTitleBarMouseDown={onTitleBarMouseDown} welcome={welcome} />;
}
