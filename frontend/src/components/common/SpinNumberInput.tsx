/**
 * Vertical spin number input — raised chrome, white value box, pixel chevrons.
 * Matches Gamedev sprite size control language.
 */

import { useEffect, useId, useState } from 'react';
import { clampAndSnap, formatValue } from './Slider';
import { nudgeByDisplayPrecision } from '../../hooks/arrowNudge';
import { useHapticFeedback } from '../../hooks/useHapticFeedback';
import { bind } from '../../shared/ui/cn';
import styles from './SpinNumberInput.module.css';

const cn = bind(styles);

interface SpinNumberInputProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  disabled?: boolean;
  enableHaptic?: boolean;
  testId?: string;
}

export default function SpinNumberInput({
  label,
  value,
  min,
  max,
  step = 1,
  onChange,
  disabled = false,
  enableHaptic = true,
  testId,
}: SpinNumberInputProps) {
  const id = useId();
  const [text, setText] = useState(() => formatValue(value, 0));
  const [editing, setEditing] = useState(false);
  const { triggerHaptic } = useHapticFeedback();

  useEffect(() => {
    if (!editing) {
      setText(formatValue(value, 0));
    }
  }, [value, editing]);

  function commitText() {
    const parsed = parseFloat(text.trim());
    if (Number.isNaN(parsed)) {
      setText(formatValue(value, 0));
      setEditing(false);
      return;
    }
    const clamped = clampAndSnap(parsed, min, max, step);
    onChange(clamped);
    setText(formatValue(clamped, 0));
    setEditing(false);
  }

  function stepBy(direction: 1 | -1) {
    if (disabled) return;
    const next = clampAndSnap(value + direction * step, min, max, step);
    if (next === value) return;
    onChange(next);
    if (enableHaptic) triggerHaptic('alignment');
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter') {
      commitText();
      (e.target as HTMLInputElement).blur();
      return;
    }
    if (e.key === 'Escape') {
      setText(formatValue(value, 0));
      setEditing(false);
      (e.target as HTMLInputElement).blur();
      return;
    }
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;

    e.preventDefault();
    e.stopPropagation();
    if (disabled) return;

    const displayed = editing ? text : formatValue(value, 0);
    const direction = e.key === 'ArrowUp' ? 1 : -1;
    const nudged = nudgeByDisplayPrecision(displayed, direction, min, max);
    if (!nudged) return;

    setEditing(true);
    setText(nudged.text);
    onChange(nudged.value);
    if (enableHaptic && nudged.value !== value) {
      triggerHaptic('alignment');
    }
  }

  return (
    <div className={cn('spinWrap')}>
      <label className={cn('spinLabel')} htmlFor={id}>
        {label}
      </label>
      <div className={cn('spin')} data-testid={testId}>
        <button
          type="button"
          className={cn('spinBtn')}
          aria-label={`Increase ${label}`}
          disabled={disabled || value >= max}
          tabIndex={-1}
          onClick={() => stepBy(1)}
        >
          <span className={cn('spinChevron', 'spinChevronUp')} aria-hidden />
        </button>
        <input
          id={id}
          className={cn('spinValue')}
          type="text"
          inputMode="numeric"
          value={editing ? text : formatValue(value, 0)}
          disabled={disabled}
          aria-label={label}
          onChange={(e) => setText(e.target.value)}
          onFocus={() => {
            setText(formatValue(value, 0));
            setEditing(true);
          }}
          onBlur={() => commitText()}
          onKeyDown={handleKeyDown}
        />
        <button
          type="button"
          className={cn('spinBtn')}
          aria-label={`Decrease ${label}`}
          disabled={disabled || value <= min}
          tabIndex={-1}
          onClick={() => stepBy(-1)}
        >
          <span className={cn('spinChevron')} aria-hidden />
        </button>
      </div>
    </div>
  );
}
