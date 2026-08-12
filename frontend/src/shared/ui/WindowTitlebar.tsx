import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from './WindowTitlebar.module.css';
import { bind } from './cn';
import type { DockSide } from '../../types/panels';

const cn = bind(styles);

type WindowTitlebarProps = {
  title: string;
  onMouseDown?: (e: React.MouseEvent) => void;
  className?: string;
  style?: React.CSSProperties;
  /** When set with onMoveToSide, right-click offers move-to-other-sidebar. */
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
};

type MenuPos = { x: number; y: number };

/** Shared retro window titlebar chrome (square + lines + title). */
export default function WindowTitlebar({
  title,
  onMouseDown,
  className,
  style,
  dockSide,
  onMoveToSide,
}: WindowTitlebarProps) {
  const otherSide: DockSide | null =
    dockSide && onMoveToSide ? (dockSide === 'left' ? 'right' : 'left') : null;
  const [menu, setMenu] = useState<MenuPos | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const closeMenu = useCallback(() => setMenu(null), []);

  useEffect(() => {
    if (!menu) return;
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current?.contains(e.target as Node)) return;
      closeMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeMenu();
    };
    document.addEventListener('mousedown', onPointer);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onPointer);
      document.removeEventListener('keydown', onKey);
    };
  }, [menu, closeMenu]);

  const handleContextMenu = (e: React.MouseEvent) => {
    if (!otherSide || !onMoveToSide) return;
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  return (
    <div
      className={cn('window-titlebar', className)}
      style={style}
      onMouseDown={onMouseDown}
      onContextMenu={handleContextMenu}
      title={
        otherSide
          ? 'Drag to other sidebar, or right-click to move'
          : undefined
      }
    >
      <div className={cn('window-titlebar-square')} />
      <div className={cn('window-titlebar-lines')} />
      <span className={cn('window-title')}>{title}</span>
      <div className={cn('window-titlebar-lines')} />
      <div className={cn('window-titlebar-square')} />

      {menu &&
        otherSide &&
        createPortal(
          <div
            ref={menuRef}
            className={cn('window-titlebar-menu')}
            role="menu"
            style={{ left: menu.x, top: menu.y }}
          >
            <button
              type="button"
              role="menuitem"
              className={cn('window-titlebar-menu-item')}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                onMoveToSide?.(otherSide);
                closeMenu();
              }}
            >
              {otherSide === 'left' ? 'Move to left sidebar' : 'Move to right sidebar'}
            </button>
          </div>,
          document.body
        )}
    </div>
  );
}
