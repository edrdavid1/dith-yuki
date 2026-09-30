import {
  useLayoutEffect,
  useRef,
  useState,
  useContext,
  type MouseEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { ShellContext } from '../../app/shell/ShellContext';
import styles from './Tooltip.module.css';
import { bind } from './cn';

const cn = bind(styles);

const OFFSET = 14;
const PAD = 6;
const MAX_WIDTH = 240;

type Point = { x: number; y: number };
type Bounds = { left: number; top: number; right: number; bottom: number };
type Size = { width: number; height: number };

function clampBounds(host: HTMLElement | null, view: Window): Bounds {
  const vw = view.innerWidth;
  const vh = view.innerHeight;
  const dock = host?.closest<HTMLElement>('[data-dock-window]');
  if (dock) {
    const r = dock.getBoundingClientRect();
    return {
      left: Math.max(PAD, r.left + PAD),
      top: Math.max(PAD, r.top + PAD),
      right: Math.min(vw - PAD, r.right - PAD),
      bottom: Math.min(vh - PAD, r.bottom - PAD),
    };
  }
  return {
    left: PAD,
    top: PAD,
    right: vw - PAD,
    bottom: vh - PAD,
  };
}

function placeTooltip(
  cursor: Point,
  size: Size,
  bounds: Bounds
): { left: number; top: number; maxWidth: number } {
  const availW = Math.max(64, bounds.right - bounds.left);
  const maxWidth = Math.min(MAX_WIDTH, availW);
  const tw = Math.min(Math.max(size.width, 1), maxWidth);
  const th = Math.max(size.height, 1);

  let x = cursor.x + OFFSET;
  let y = cursor.y + OFFSET;

  if (x + tw > bounds.right) x = cursor.x - OFFSET - tw;
  if (x < bounds.left) x = bounds.left;
  if (x + tw > bounds.right) x = Math.max(bounds.left, bounds.right - tw);

  if (y + th > bounds.bottom) y = cursor.y - OFFSET - th;
  if (y < bounds.top) y = bounds.top;
  if (y + th > bounds.bottom) y = Math.max(bounds.top, bounds.bottom - th);

  return { left: x, top: y, maxWidth };
}

/**
 * Cursor-following label. Clamps to the dock panel / window.
 *
 * Geometry is applied in useLayoutEffect (and on mousemove) via the DOM node.
 * That way Color Lab re-renders can't flash the tip back to a hidden/-9999
 * React style and cause flicker.
 */
export default function Tooltip({
  label,
  children,
  fill = false,
}: {
  label: string;
  children: ReactNode;
  fill?: boolean;
}) {
  const hostRef = useRef<HTMLSpanElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef<Size>({ width: 0, height: 0 });
  const cursorRef = useRef<Point>({ x: 0, y: 0 });
  const [open, setOpen] = useState(false);
  const shell = useContext(ShellContext);
  const tooltipsEnabled = shell?.tooltipsEnabled ?? true;

  const writePosition = () => {
    const tip = tipRef.current;
    const host = hostRef.current;
    if (!tip) return;

    const doc = host?.ownerDocument ?? document;
    const view = doc.defaultView ?? window;
    const bounds = clampBounds(host, view);
    const maxWidth = Math.min(MAX_WIDTH, Math.max(64, bounds.right - bounds.left));

    if (sizeRef.current.width <= 0) {
      tip.style.visibility = 'hidden';
      tip.style.left = '-9999px';
      tip.style.top = '0px';
      tip.style.maxWidth = `${maxWidth}px`;
      const w = tip.offsetWidth;
      const h = tip.offsetHeight;
      sizeRef.current = { width: w > 0 ? w : 1, height: h > 0 ? h : 1 };
    } else {
      const w = tip.offsetWidth;
      const h = tip.offsetHeight;
      // Only trust on-screen measurements (ignore a transient -9999 layout).
      if (w > 0 && h > 0 && tip.style.left !== '-9999px') {
        sizeRef.current = { width: w, height: h };
      }
    }

    const pos = placeTooltip(cursorRef.current, sizeRef.current, bounds);
    tip.style.left = `${pos.left}px`;
    tip.style.top = `${pos.top}px`;
    tip.style.maxWidth = `${pos.maxWidth}px`;
    tip.style.visibility = 'visible';
  };

  const onEnter = (e: MouseEvent) => {
    if (!tooltipsEnabled) return;
    cursorRef.current = { x: e.clientX, y: e.clientY };
    sizeRef.current = { width: 0, height: 0 };
    setOpen(true);
  };

  const onMove = (e: MouseEvent) => {
    if (!tooltipsEnabled) return;
    cursorRef.current = { x: e.clientX, y: e.clientY };
    if (open) writePosition();
  };

  const onLeave = () => {
    sizeRef.current = { width: 0, height: 0 };
    setOpen(false);
  };

  // Close if the user turns tooltips off while one is showing.
  useLayoutEffect(() => {
    if (!tooltipsEnabled && open) {
      sizeRef.current = { width: 0, height: 0 };
      setOpen(false);
    }
  }, [tooltipsEnabled, open]);

  // Re-apply after every commit while open so parent re-renders can't leave the
  // tip stuck at the default hidden/-9999 styles from JSX.
  useLayoutEffect(() => {
    if (!open) return;
    writePosition();
  });

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
      {open &&
        createPortal(
          <div ref={tipRef} className={cn('tooltip')} role="tooltip">
            {label}
          </div>,
          portalParent
        )}
    </span>
  );
}
