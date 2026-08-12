import { useState, useEffect, useRef, useCallback } from 'react';
import styles from '../../shared/ui/Slider.module.css';
import retro from '../../shared/ui/RetroSlider.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind({ ...styles, ...retro });


interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  /** Number of decimal places to display (default: 2) */
  decimals?: number;
}

export function formatValue(value: number, decimals: number = 2): string {
  return value.toFixed(decimals);
}

function clampAndSnap(raw: number, min: number, max: number, step: number): number {
  let clamped = raw;
  if (clamped < min) clamped = min;
  if (clamped > max) clamped = max;
  const stepMul = 1 / step;
  return Math.round(clamped * stepMul) / stepMul;
}

function Slider({ label, value, min, max, step, onChange, decimals = 2 }: SliderProps) {
  // localValue is the source of truth for visual display.
  // It updates immediately on drag and syncs from props when props change externally.
  const [localValue, setLocalValue] = useState(value);
  const [text, setText] = useState(() => formatValue(value, decimals));
  const [editing, setEditing] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);

  // Sync from props — only when value prop changes from outside (not from our own onChange)
  const lastEmittedRef = useRef(value);
  useEffect(() => {
    // If the prop value changed and it wasn't us who caused it, sync
    if (value !== lastEmittedRef.current) {
      lastEmittedRef.current = value;
      if (!draggingRef.current) {
        setLocalValue(value);
        if (!editing) {
          setText(formatValue(value, decimals));
        }
      }
    }
  }, [value, decimals, editing]);

  // Use refs so the mousemove closure always has fresh values
  const minRef = useRef(min);
  const maxRef = useRef(max);
  const stepRef = useRef(step);
  const onChangeRef = useRef(onChange);
  const decimalsRef = useRef(decimals);
  minRef.current = min;
  maxRef.current = max;
  stepRef.current = step;
  onChangeRef.current = onChange;
  decimalsRef.current = decimals;

  // Compute value from mouse X position relative to track
  const valueFromMouseX = useCallback((clientX: number): number => {
    const track = trackRef.current;
    if (!track) return localValue;
    const rect = track.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return clampAndSnap(
      minRef.current + ratio * (maxRef.current - minRef.current),
      minRef.current,
      maxRef.current,
      stepRef.current
    );
  }, [localValue]);

  // Mouse down on track or thumb — start drag
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    draggingRef.current = true;

    const newVal = valueFromMouseX(e.clientX);
    setLocalValue(newVal);
    setText(formatValue(newVal, decimalsRef.current));
    lastEmittedRef.current = newVal;
    onChangeRef.current(newVal);

    const handleMouseMove = (ev: MouseEvent) => {
      const val = valueFromMouseX(ev.clientX);
      setLocalValue(val);
      setText(formatValue(val, decimalsRef.current));
      lastEmittedRef.current = val;
      onChangeRef.current(val);
    };

    const handleMouseUp = () => {
      draggingRef.current = false;
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [valueFromMouseX]);

  // Thumb position as percentage
  const percent = max > min ? ((localValue - min) / (max - min)) * 100 : 0;

  function commitText() {
    const raw = text.trim().replace('%', '');
    const parsed = parseFloat(raw);
    if (Number.isNaN(parsed)) {
      setText(formatValue(localValue, decimals));
      setEditing(false);
      return;
    }
    const clamped = clampAndSnap(parsed, min, max, step);
    setLocalValue(clamped);
    lastEmittedRef.current = clamped;
    onChange(clamped);
    setText(formatValue(clamped, decimals));
    setEditing(false);
  }

  return (
    <div className={cn("slider-control")}>
      <label className={cn("slider-label")}>{label}</label>
      <div className={cn("slider-row")}>
        <div
          className={cn("retro-slider-track")}
          ref={trackRef}
          onMouseDown={handleMouseDown}
        >
          <div
            className={cn("retro-slider-thumb")}
            style={{ left: `${percent}%` }}
          >
            <img src="/icons/slider-carrete-icon.svg" width="16" height="35" alt="" draggable={false} />
          </div>
        </div>
        <input
          className={cn("slider-value-box")}
          type="text"
          value={editing ? text : formatValue(localValue, decimals)}
          onChange={(e) => setText(e.target.value)}
          onFocus={() => setEditing(true)}
          onBlur={() => commitText()}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              commitText();
              (e.target as HTMLInputElement).blur();
            } else if (e.key === 'Escape') {
              setText(formatValue(localValue, decimals));
              setEditing(false);
              (e.target as HTMLInputElement).blur();
            }
          }}
        />
      </div>
    </div>
  );
}

export default Slider;
