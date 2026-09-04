import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from './WindowTitlebar.module.css';
import { bind } from './cn';
import Tooltip from './Tooltip';
import type { DockSide } from '../../types/panels';

const cn = bind(styles);

export type WindowTitlebarVariant = 'docked' | 'floating' | 'dialog';

type WindowTitlebarProps = {
  title: string;
  /** docked = □ stripes Title stripes □ ; floating = close/min/zoom + stripes Title */
  variant?: WindowTitlebarVariant;
  onMouseDown?: (e: React.MouseEvent) => void;
  className?: string;
  style?: React.CSSProperties;
  /** When set with onMoveToSide, menu offers move-to-other-sidebar. */
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
  /** Pop the panel into a separate OS window (FlexLayout float). */
  onPopOut?: () => void;
  /** Dock a floated panel back into the sidebar / close floating chrome. */
  onDockBack?: () => void;
  /** Floating variant: close (dock back or dismiss). */
  onClose?: () => void;
  /** Floating variant: minimize OS window. */
  onMinimize?: () => void;
  /** Floating variant: maximize / zoom. */
  onMaximize?: () => void;
  /** Tooltip / aria for the close control. */
  closeLabel?: string;
};

type MenuPos = { x: number; y: number; doc: Document };

function TitleStripes({ title, titleId }: { title: string; titleId?: string }) {
  return (
    <>
      <div className={cn('window-titlebar-lines')} aria-hidden />
      <span className={cn('window-title')} id={titleId}>
        {title}
      </span>
      <div className={cn('window-titlebar-lines')} aria-hidden />
    </>
  );
}

export function DialogTitlebar({
  title,
  titleId,
  onClose,
}: {
  title: string;
  titleId?: string;
  onClose: () => void;
}) {
  return (
    <div className={cn('window-titlebar', 'window-titlebar--dialog')}>
      <button
        type="button"
        className={cn('window-titlebar-close')}
        onClick={onClose}
        aria-label="Close"
      >
        <img src="/icons/clouse-window-icon.svg" width="14" height="14" alt="" />
      </button>
      <TitleStripes title={title} titleId={titleId} />
      <div className={cn('window-titlebar-square')} aria-hidden />
    </div>
  );
}

/**
 * Figma `heder-of-window` (DithTom 36:154) — shared docked + floating chrome.
 * Height 20px, Chicago title, horizontal ridges, square controls.
 */
export default function WindowTitlebar({
  title,
  variant = 'docked',
  onMouseDown,
  className,
  style,
  dockSide,
  onMoveToSide,
  onPopOut,
  onDockBack,
  onClose,
  onMinimize,
  onMaximize,
  closeLabel,
}: WindowTitlebarProps) {
  const isFloating = variant === 'floating';
  const otherSide: DockSide | null =
    dockSide && onMoveToSide ? (dockSide === 'left' ? 'right' : 'left') : null;
  const hasMenu = Boolean(otherSide || onPopOut || onDockBack);
  const [menu, setMenu] = useState<MenuPos | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const closeMenu = useCallback(() => setMenu(null), []);

  useEffect(() => {
    if (!menu) return;
    const doc = menu.doc;
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current?.contains(e.target as Node)) return;
      closeMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeMenu();
    };
    doc.addEventListener('mousedown', onPointer);
    doc.addEventListener('keydown', onKey);
    return () => {
      doc.removeEventListener('mousedown', onPointer);
      doc.removeEventListener('keydown', onKey);
    };
  }, [menu, closeMenu]);

  const openMenuAt = (x: number, y: number, doc: Document) => {
    if (!hasMenu) return;
    setMenu({ x, y, doc });
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    if (!hasMenu) return;
    e.preventDefault();
    e.stopPropagation();
    openMenuAt(e.clientX, e.clientY, e.currentTarget.ownerDocument);
  };

  const handleSquareClick = (e: React.MouseEvent) => {
    if (!hasMenu) return;
    e.preventDefault();
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenuAt(rect.left, rect.bottom + 2, e.currentTarget.ownerDocument);
  };

  const handleSquareMouseDown = (e: React.MouseEvent) => {
    if (!hasMenu) return;
    e.stopPropagation();
  };

  let squareHint = 'Panel menu';
  if (otherSide && onPopOut) {
    squareHint = 'Move or open in separate window';
  } else if (otherSide && onDockBack) {
    squareHint = 'Move or dock back';
  } else if (onPopOut) {
    squareHint = 'Open in separate window';
  } else if (onDockBack) {
    squareHint = 'Dock back to sidebar';
  } else if (otherSide) {
    squareHint = 'Move to other sidebar';
  }

  const resolvedCloseLabel =
    closeLabel ??
    (onDockBack || onClose ? 'Dock panel back to sidebar' : 'Close');

  const handleCloseClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    (onClose ?? onDockBack)?.();
  };

  return (
    <div
      className={cn(
        'window-titlebar',
        isFloating && 'window-titlebar--floating',
        className,
      )}
      style={style}
      onMouseDown={onMouseDown}
      onContextMenu={handleContextMenu}
      data-window-titlebar
      data-variant={variant}
    >
      {isFloating ? (
        <div className={cn('window-titlebar-actions')}>
          <Tooltip label={resolvedCloseLabel} fill>
            <button
              type="button"
              className={cn('window-titlebar-close')}
              onClick={handleCloseClick}
              onMouseDown={(e) => e.stopPropagation()}
              aria-label={resolvedCloseLabel}
            >
              <img src="/icons/clouse-window-icon.svg" width="14" height="14" alt="" />
            </button>
          </Tooltip>
          {onMinimize && (
            <Tooltip label="Minimize" fill>
              <button
                type="button"
                className={cn('window-titlebar-min')}
                onClick={(e) => {
                  e.stopPropagation();
                  onMinimize();
                }}
                onMouseDown={(e) => e.stopPropagation()}
                aria-label="Minimize"
              >
                <img src="/icons/hide-window-icon.svg" width="14" height="14" alt="" />
              </button>
            </Tooltip>
          )}
          {onMaximize && (
            <Tooltip label="Zoom" fill>
              <button
                type="button"
                className={cn('window-titlebar-zoom')}
                onClick={(e) => {
                  e.stopPropagation();
                  onMaximize();
                }}
                onMouseDown={(e) => e.stopPropagation()}
                aria-label="Zoom"
              >
                <img src="/icons/header-window-square-icon.svg" width="14" height="14" alt="" />
              </button>
            </Tooltip>
          )}
        </div>
      ) : (
        <Tooltip label={squareHint} fill>
          <button
            type="button"
            className={cn('window-titlebar-square')}
            aria-label="Panel menu"
            onMouseDown={handleSquareMouseDown}
            onClick={handleSquareClick}
          />
        </Tooltip>
      )}

      <TitleStripes title={title} />

      {!isFloating && (
        <Tooltip label={squareHint} fill>
          <button
            type="button"
            className={cn('window-titlebar-square')}
            aria-label="Panel menu"
            onMouseDown={handleSquareMouseDown}
            onClick={handleSquareClick}
          />
        </Tooltip>
      )}

      {menu &&
        hasMenu &&
        createPortal(
          <div
            ref={menuRef}
            className={cn('window-titlebar-menu')}
            role="menu"
            style={{ left: menu.x, top: menu.y }}
          >
            {otherSide && onMoveToSide && (
              <button
                type="button"
                role="menuitem"
                className={cn('window-titlebar-menu-item')}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onMoveToSide(otherSide);
                  closeMenu();
                }}
              >
                {otherSide === 'left' ? 'Move to left sidebar' : 'Move to right sidebar'}
              </button>
            )}
            {onPopOut && (
              <button
                type="button"
                role="menuitem"
                className={cn('window-titlebar-menu-item')}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onPopOut();
                  closeMenu();
                }}
              >
                Open in separate window
              </button>
            )}
            {onDockBack && (
              <button
                type="button"
                role="menuitem"
                className={cn('window-titlebar-menu-item')}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onDockBack();
                  closeMenu();
                }}
              >
                Dock back to sidebar
              </button>
            )}
          </div>,
          menu.doc.body
        )}
    </div>
  );
}
