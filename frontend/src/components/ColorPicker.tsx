import { useState, useEffect, useRef, useCallback } from 'react';
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

function ColorPicker({ initialColor, onConfirm, onCancel, anchorRect }: ColorPickerProps) {
  const defaultHex = initialColor ?? 'FFFFFF';
  const [color, setColor] = useState(`#${defaultHex}`);
  const [hexInput, setHexInput] = useState(defaultHex.toUpperCase());
  const modalRef = useRef<HTMLDivElement>(null);

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

  // Position the popup near the anchor
  const style: React.CSSProperties = {
    position: 'fixed',
    zIndex: 10000,
  };
  if (anchorRect) {
    style.top = anchorRect.bottom + 4;
    style.left = anchorRect.left;
  } else {
    style.top = '50%';
    style.left = '50%';
    style.transform = 'translate(-50%, -50%)';
  }

  return (
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
    </div>
  );
}

export default ColorPicker;
