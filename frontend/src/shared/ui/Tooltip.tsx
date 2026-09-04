import { useCallback, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import styles from './Tooltip.module.css';
import { bind } from './cn';

const cn = bind(styles);

const OFFSET = 14;

/**
 * Cursor-following label. Use instead of native `title` on icon-only controls.
 * Portals into the host element's document (works in FlexLayout popout windows).
 */
export default function Tooltip({
  label,
  children,
  fill = false,
}: {
  label: string;
  children: ReactNode;
  /** Stretch host to fill a flex row (e.g. titlebar square). */
  fill?: boolean;
}) {
  const hostRef = useRef<HTMLSpanElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  const onEnter = useCallback((e: MouseEvent) => {
    setPos({ x: e.clientX + OFFSET, y: e.clientY + OFFSET });
  }, []);

  const onMove = useCallback((e: MouseEvent) => {
    setPos({ x: e.clientX + OFFSET, y: e.clientY + OFFSET });
  }, []);

  const onLeave = useCallback(() => setPos(null), []);

  const portalParent =
    hostRef.current?.ownerDocument?.body ?? document.body;

  return (
    <span
      ref={hostRef}
      data-tooltip-host
      className={cn('tooltip-host', fill && 'tooltip-host-fill')}
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
          portalParent
        )}
    </span>
  );
}
