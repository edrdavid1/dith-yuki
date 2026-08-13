import { useState, useEffect, useId } from 'react';
import { clampAndSnap, formatValue } from './Slider';
import sliderStyles from '../../shared/ui/Slider.module.css';
import inputStyles from '../../shared/ui/ParamInput.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...sliderStyles, ...inputStyles });

// Same clamp/commit as Slider. No local debounce — IPC coalescing is
// useEffectLayer.updateParams (100ms). Enter/blur commits immediately.

interface NumberInputProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  /** Number of decimal places to display (default: 0, integer). */
  decimals?: number;
  /** Inline field without a visible stacked label (curve point rows). */
  compact?: boolean;
  disabled?: boolean;
}

function NumberInput({
  label,
  value,
  min,
  max,
  step,
  onChange,
  decimals = 0,
  compact = false,
  disabled = false,
}: NumberInputProps) {
  const id = useId();
  const [text, setText] = useState(() => formatValue(value, decimals));
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    if (!editing) {
      setText(formatValue(value, decimals));
    }
  }, [value, decimals, editing]);

  function commitText() {
    const raw = text.trim().replace('%', '');
    const parsed = parseFloat(raw);
    if (Number.isNaN(parsed)) {
      setText(formatValue(value, decimals));
      setEditing(false);
      return;
    }
    const clamped = clampAndSnap(parsed, min, max, step);
    onChange(clamped);
    setText(formatValue(clamped, decimals));
    setEditing(false);
  }

  return (
    <div className={cn(compact ? 'param-input-compact' : 'slider-control')}>
      <label
        className={cn(compact ? 'param-input-compact-label' : 'slider-label')}
        htmlFor={id}
      >
        {label}
      </label>
      <input
        id={id}
        className={cn(compact ? 'param-input-compact-field' : 'param-input')}
        type="text"
        inputMode={decimals > 0 ? 'decimal' : 'numeric'}
        value={editing ? text : formatValue(value, decimals)}
        disabled={disabled}
        onChange={(e) => setText(e.target.value)}
        onFocus={() => setEditing(true)}
        onBlur={() => commitText()}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            commitText();
            (e.target as HTMLInputElement).blur();
          } else if (e.key === 'Escape') {
            setText(formatValue(value, decimals));
            setEditing(false);
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
    </div>
  );
}

export default NumberInput;
