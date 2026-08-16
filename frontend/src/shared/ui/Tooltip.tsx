import { useCallback, useState, type MouseEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import styles from './Tooltip.module.css';
import { bind } from './cn';

const cn = bind(styles);

const OFFSET = 14;

/**
 * Cursor-following label. Use instead of native `title` on icon-only controls.
 */
export default function Tooltip({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  const onEnter = useCallback((e: MouseEvent) => {
    setPos({ x: e.clientX + OFFSET, y: e.clientY + OFFSET });
  }, []);

  const onMove = useCallback((e: MouseEvent) => {
    setPos({ x: e.clientX + OFFSET, y: e.clientY + OFFSET });
  }, []);

  const onLeave = useCallback(() => setPos(null), []);

  return (
    <span
      className={cn('tooltip-host')}
      onMouseEnter={onEnter}
      onMouseMove={onMove}
      onMouseLeave={onLeave}
    >
      {children}
      {pos &&
        createPortal(
          <div
            className={cn('tooltip')}
            role="tooltip"
            style={{ left: pos.x, top: pos.y }}
          >
            {label}
          </div>,
          document.body
        )}
    </span>
  );
}
