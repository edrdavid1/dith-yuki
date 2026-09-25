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
}

const POPUP_WIDTH = 220;
const POPUP_HEIGHT = 220;
const VIEWPORT_PAD = 8;

function clampPopupPosition(anchor: DOMRect | null | undefined): React.CSSProperties {
  const style: React.CSSProperties = {
    position: 'fixed',
    zIndex: 10000,
  };
  if (!anchor) {
    style.top = '50%';
    style.left = '50%';
    style.transform = 'translate(-50%, -50%)';
    return style;
  }

  let top = anchor.bottom + 4;
  let left = anchor.left;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  if (left + POPUP_WIDTH > vw - VIEWPORT_PAD) {
    left = Math.max(VIEWPORT_PAD, vw - POPUP_WIDTH - VIEWPORT_PAD);
  }
  if (left < VIEWPORT_PAD) left = VIEWPORT_PAD;

  if (top + POPUP_HEIGHT > vh - VIEWPORT_PAD) {
    // Flip above the anchor when it would overflow the bottom.
    top = Math.max(VIEWPORT_PAD, anchor.top - POPUP_HEIGHT - 4);
  }

  style.top = top;
  style.left = left;
  return style;
}

function ColorPicker({ initialColor, onConfirm, onCancel, anchorRect }: ColorPickerProps) {
  const defaultHex = initialColor ?? 'FFFFFF';
  const [color, setColor] = useState(`#${defaultHex}`);
  const [hexInput, setHexInput] = useState(defaultHex.toUpperCase());
  const [style, setStyle] = useState<React.CSSProperties>(() => clampPopupPosition(anchorRect));
  const modalRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    setStyle(clampPopupPosition(anchorRect));
  }, [anchorRect]);

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

  // Handle Escape key — close picker
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      }
    };
    document.addEventListener('keydown', handleKeyDown, true);
    return () => document.removeEventListener('keydown', handleKeyDown, true);
  }, [onCancel]);

  // Handle click outside the picker popup — close it
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        onCancel();
      }
    };
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside);
    }, 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onCancel]);

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
    document.body
  );
}

export default ColorPicker;
