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

function Slider({ label, value, min, max, step, onChange, decimals = 2 }: SliderProps) {
  return (
    <div className="slider-control">
      <div className="slider-header">
        <label className="slider-label">{label}</label>
        <span className="slider-value">{formatValue(value, decimals)}</span>
      </div>
      <input
        type="range"
        className="slider-input"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
      />
    </div>
  );
}

export default Slider;
