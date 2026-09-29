import { useState, useEffect, useRef, useCallback } from 'react';
import styles from '../../shared/ui/Slider.module.css';
import retro from '../../shared/ui/RetroSlider.module.css';
import { bind } from '../../shared/ui/cn';
import { useHapticFeedback } from '../../hooks/useHapticFeedback';
import {
  createHapticMotionState,
  evaluateSliderHaptic,
  type HapticMotionState,
} from '../../hooks/sliderHaptics';
import { nudgeByDisplayPrecision } from '../../hooks/arrowNudge';
const cn = bind({ ...styles, ...retro });

// Immediate onChange for local/parent state. IPC debounce (100ms default;
// 350ms for ASCII / Riemersma) lives in useEffectLayer.updateParams — do not
// add a second timer here (would stack delays).
// Text field Enter/blur commits immediately (bypass; still one IPC layer).

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  /** Number of decimal places to display (default: 2) */
  decimals?: number;
  /** Taptic feedback while dragging (honors Preferences → Tactile feedback). */
  enableHaptic?: boolean;
}

export function formatValue(value: number, decimals: number = 2): string {
  return value.toFixed(decimals);
}

export function clampAndSnap(raw: number, min: number, max: number, step: number): number {
  let clamped = raw;
  if (clamped < min) clamped = min;
  if (clamped > max) clamped = max;
  const stepMul = 1 / step;
  return Math.round(clamped * stepMul) / stepMul;
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  onChange,
  decimals = 2,
  enableHaptic = true,
}: SliderProps) {
  // localValue is the source of truth for visual display.
  // It updates immediately on drag and syncs from props when props change externally.
  const [localValue, setLocalValue] = useState(value);
  const [text, setText] = useState(() => formatValue(value, decimals));
  const [editing, setEditing] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const hapticMotionRef = useRef<HapticMotionState>(createHapticMotionState(value));
  const { triggerHaptic } = useHapticFeedback();

  // Sync from props — only when value prop changes from outside (not from our own onChange)
  const lastEmittedRef = useRef(value);
  useEffect(() => {
    // If the prop value changed and it wasn't us who caused it, sync
    if (value !== lastEmittedRef.current) {
      lastEmittedRef.current = value;
      if (!draggingRef.current) {
        setLocalValue(value);
        hapticMotionRef.current = createHapticMotionState(value);
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
  const enableHapticRef = useRef(enableHaptic);
  const triggerHapticRef = useRef(triggerHaptic);
  minRef.current = min;
  maxRef.current = max;
  stepRef.current = step;
  onChangeRef.current = onChange;
  decimalsRef.current = decimals;
  enableHapticRef.current = enableHaptic;
  triggerHapticRef.current = triggerHaptic;

  const applyUserValue = useCallback((prev: number, next: number) => {
    setLocalValue(next);
    setText(formatValue(next, decimalsRef.current));
    lastEmittedRef.current = next;
    onChangeRef.current(next);

    if (!enableHapticRef.current) return;
    const decision = evaluateSliderHaptic({
      prev,
      next,
      min: minRef.current,
      max: maxRef.current,
      now: performance.now(),
      state: hapticMotionRef.current,
    });
    hapticMotionRef.current = decision.state;
    if (decision.trigger && decision.pattern) {
      // Fire-and-forget — never await.
      triggerHapticRef.current(decision.pattern);
    }
  }, []);

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

    const from = lastEmittedRef.current;
    hapticMotionRef.current = createHapticMotionState(from);
    const newVal = valueFromMouseX(e.clientX);
    applyUserValue(from, newVal);

    const handleMouseMove = (ev: MouseEvent) => {
      const prev = lastEmittedRef.current;
      const val = valueFromMouseX(ev.clientX);
      applyUserValue(prev, val);
    };

    const handleMouseUp = () => {
      draggingRef.current = false;
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [applyUserValue, valueFromMouseX]);

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
    hapticMotionRef.current = createHapticMotionState(clamped);
    onChange(clamped);
    setText(formatValue(clamped, decimals));
    setEditing(false);
  }

  function handleValueKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter') {
      commitText();
      (e.target as HTMLInputElement).blur();
      return;
    }
    if (e.key === 'Escape') {
      setText(formatValue(localValue, decimals));
      setEditing(false);
      (e.target as HTMLInputElement).blur();
      return;
    }
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;

    e.preventDefault();
    e.stopPropagation();

    const displayed = editing ? text : formatValue(localValue, decimals);
    const direction = e.key === 'ArrowUp' ? 1 : -1;
    const nudged = nudgeByDisplayPrecision(displayed, direction, min, max);
    if (!nudged) return;

    const prev = lastEmittedRef.current;
    setEditing(true);
    setLocalValue(nudged.value);
    setText(nudged.text);
    lastEmittedRef.current = nudged.value;
    hapticMotionRef.current = createHapticMotionState(nudged.value);
    onChange(nudged.value);

    if (enableHaptic && nudged.value !== prev) {
      triggerHaptic('alignment');
    }
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
          inputMode="decimal"
          aria-label={`${label} value`}
          value={editing ? text : formatValue(localValue, decimals)}
          onChange={(e) => setText(e.target.value)}
          onFocus={() => {
            setText(formatValue(localValue, decimals));
            setEditing(true);
          }}
          onBlur={() => commitText()}
          onKeyDown={handleValueKeyDown}
        />
      </div>
    </div>
  );
}

export default Slider;
