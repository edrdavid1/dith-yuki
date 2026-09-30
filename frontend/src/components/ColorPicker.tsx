import { useState, useEffect, useRef, useCallback, useLayoutEffect } from 'react';
import { createPortal } from 'react-dom';
import { HexColorPicker } from 'react-colorful';
import styles from '../features/color-lab/ColorPicker.module.css';
import { bind } from '../shared/ui/cn';
const cn = bind(styles);

interface ColorPickerProps {
  initialColor?: string; // 6-char hex, e.g. "FF0000"
  /** Called on every color change (live update) */
  onConfirm: (hex: string) => void;
  onCancel: () => void;
  /** Position anchor — the bounding rect of the trigger element */
  anchorRect?: DOMRect | null;
  /**
   * Document body (or other host) that should own the popup.
   * Required when Color Lab is in a FlexLayout OS popout: React still runs in
   * the main window, but the panel DOM lives in the popout document — portaling
   * to `document.body` would put the picker in the wrong window with wrong
   * coordinates. Pass `event.currentTarget.ownerDocument.body` from the opener.
   */
  portalRoot?: Element | null;
}

/** Compact size so the popup fits a ~332px Color Lab sidebar / narrow popout. */
const POPUP_WIDTH = 168;
const POPUP_HEIGHT = 168;
const VIEWPORT_PAD = 6;
/** Above FlexLayout chrome (10_000) and titlebar menus (99_999). */
const POPUP_Z_INDEX = 100_000;

type Bounds = { left: number; top: number; right: number; bottom: number };

function resolvePortalDocument(portalRoot: Element | null | undefined): Document {
  if (portalRoot) {
    return portalRoot.ownerDocument ?? document;
  }
  return document;
}

/** Prefer Color Lab panel edges; fall back to the portal window viewport. */
function getClampBounds(portalDoc: Document, view: Window): Bounds {
  const vw = view.innerWidth;
  const vh = view.innerHeight;
  const lab = portalDoc.querySelector<HTMLElement>('[data-dock-window="colorlab"]');
  if (lab) {
    const r = lab.getBoundingClientRect();
    // Intersect panel with viewport so we never spill outside the OS window.
    return {
      left: Math.max(VIEWPORT_PAD, r.left + VIEWPORT_PAD),
      top: Math.max(VIEWPORT_PAD, r.top + VIEWPORT_PAD),
      right: Math.min(vw - VIEWPORT_PAD, r.right - VIEWPORT_PAD),
      bottom: Math.min(vh - VIEWPORT_PAD, r.bottom - VIEWPORT_PAD),
    };
  }
  return {
    left: VIEWPORT_PAD,
    top: VIEWPORT_PAD,
    right: vw - VIEWPORT_PAD,
    bottom: vh - VIEWPORT_PAD,
  };
}

function clampPopupPosition(
  anchor: DOMRect | null | undefined,
  bounds: Bounds,
  size: { width: number; height: number }
): React.CSSProperties {
  const availW = Math.max(0, bounds.right - bounds.left);
  const availH = Math.max(0, bounds.bottom - bounds.top);
  const width = Math.min(size.width, availW || size.width);
  const height = Math.min(size.height, availH || size.height);

  const style: React.CSSProperties = {
    position: 'fixed',
    zIndex: POPUP_Z_INDEX,
    // Cap to available space so a narrow/short Color Lab never overflows.
    maxWidth: availW > 0 ? availW : undefined,
    maxHeight: availH > 0 ? availH : undefined,
  };

  if (!anchor) {
    style.top = (bounds.top + bounds.bottom) / 2;
    style.left = (bounds.left + bounds.right) / 2;
    style.transform = 'translate(-50%, -50%)';
    return style;
  }

  const gap = 4;
  const spaceBelow = bounds.bottom - (anchor.bottom + gap);
  const spaceAbove = anchor.top - gap - bounds.top;

  let top: number;
  if (height <= spaceBelow) {
    top = anchor.bottom + gap;
  } else if (height <= spaceAbove) {
    top = anchor.top - height - gap;
  } else if (spaceBelow >= spaceAbove) {
    top = Math.max(bounds.top, bounds.bottom - height);
  } else {
    top = bounds.top;
  }

  // Prefer left-align with the swatch; if that overflows, right-align, then clamp.
  let left = anchor.left;
  if (left + width > bounds.right) {
    left = anchor.right - width;
  }
  if (left < bounds.left) left = bounds.left;
  if (left + width > bounds.right) left = Math.max(bounds.left, bounds.right - width);

  if (top < bounds.top) top = bounds.top;
  if (top + height > bounds.bottom) top = Math.max(bounds.top, bounds.bottom - height);

  style.top = top;
  style.left = left;
  return style;
}

function ColorPicker({
  initialColor,
  onConfirm,
  onCancel,
  anchorRect,
  portalRoot,
}: ColorPickerProps) {
  const defaultHex = initialColor ?? 'FFFFFF';
  const [color, setColor] = useState(`#${defaultHex}`);
  const [hexInput, setHexInput] = useState(defaultHex.toUpperCase());
  const portalDoc = resolvePortalDocument(portalRoot);
  const portalView = portalDoc.defaultView ?? window;
  const modalRef = useRef<HTMLDivElement>(null);
  const host = portalRoot ?? portalDoc.body;

  const [style, setStyle] = useState<React.CSSProperties>(() =>
    clampPopupPosition(
      anchorRect,
      getClampBounds(portalDoc, portalView),
      { width: POPUP_WIDTH, height: POPUP_HEIGHT }
    )
  );

  // Position with estimated size, then re-clamp from the real measured box so
  // we respect Color Lab / popout edges even when content is taller than the estimate.
  useLayoutEffect(() => {
    const bounds = getClampBounds(portalDoc, portalView);
    const el = modalRef.current;
    const size = el
      ? { width: el.offsetWidth, height: el.offsetHeight }
      : { width: POPUP_WIDTH, height: POPUP_HEIGHT };
    setStyle(clampPopupPosition(anchorRect, bounds, size));
  }, [anchorRect, portalDoc, portalView]);

  // Sync hex input when picker changes and emit live update
  const handlePickerChange = useCallback((newColor: string) => {
    setColor(newColor);
    const hex = newColor.replace('#', '').toUpperCase();
    setHexInput(hex);
    onConfirm(hex);
  }, [onConfirm]);

  // Sync picker when hex input changes
  const handleHexInputChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value.toUpperCase().replace(/[^0-9A-F]/g, '').slice(0, 6);
    setHexInput(value);
    if (value.length === 6) {
      setColor(`#${value}`);
      onConfirm(value);
    }
  }, [onConfirm]);

  // Handle Escape key — close picker (listen on the portal's document)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      }
    };
    portalDoc.addEventListener('keydown', handleKeyDown, true);
    return () => portalDoc.removeEventListener('keydown', handleKeyDown, true);
  }, [onCancel, portalDoc]);

  // Handle click outside the picker popup — close it
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        onCancel();
      }
    };
    const timer = setTimeout(() => {
      portalDoc.addEventListener('mousedown', handleClickOutside);
    }, 0);
    return () => {
      clearTimeout(timer);
      portalDoc.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onCancel, portalDoc]);

  return createPortal(
    <div
      className={cn("color-picker-popup")}
      ref={modalRef}
      role="dialog"
      aria-modal="true"
      aria-label="Color Picker"
      style={style}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <HexColorPicker color={color} onChange={handlePickerChange} />

      <div className={cn("color-picker-input-row")}>
        <span className={cn("color-picker-hash")}>#</span>
        <input
          className={cn("color-picker-hex-input")}
          type="text"
          value={hexInput}
          onChange={handleHexInputChange}
          maxLength={6}
          aria-label="Hex color value"
        />
        <div
          className={cn("color-picker-preview")}
          style={{ backgroundColor: color }}
          aria-label="Color preview"
        />
      </div>
    </div>,
    host
  );
}

export default ColorPicker;
