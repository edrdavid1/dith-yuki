/**
 * Placeholder shown in the docked tab body while its panel is in a FlexLayout OS popout.
 * Replaces FlexLayout's default "This panel is shown in a floating window" copy.
 */

import styles from './FloatingTabPlaceholder.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

export default function FloatingTabPlaceholder({
  onShowWindow,
  onDockBack,
}: {
  onShowWindow: () => void;
  onDockBack: () => void;
}) {
  return (
    <div className={cn('ftp')}>
      <button type="button" className={cn('ftp-btn')} onClick={onShowWindow}>
        Show window
      </button>
      <button type="button" className={cn('ftp-btn')} onClick={onDockBack}>
        Dock back
      </button>
    </div>
  );
}
