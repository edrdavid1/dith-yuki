import Slider from '../common/Slider';
import type { GlitchType } from '../../types';

interface GlitchParamsProps {
  glitchType: GlitchType;
  intensity: number;
  seed: number;
  onChange: (params: Record<string, unknown>) => void;
}

function GlitchParams({ glitchType, intensity, seed, onChange }: GlitchParamsProps) {
  return (
    <div className="filter-params">
      <div className="param-group">
        <label className="slider-label">Effect Type</label>
        <select
          className="param-select"
          value={glitchType}
          onChange={(e) => onChange({ glitch_type: e.target.value, intensity, seed })}
        >
          <option value="RGBShift">RGB Shift</option>
          <option value="BlockDisplace">Block Displacement</option>
        </select>
      </div>
      <Slider
        label="Intensity"
        value={intensity}
        min={0}
        max={1}
        step={0.01}
        onChange={(val) => onChange({ glitch_type: glitchType, intensity: val, seed })}
      />
    </div>
  );
}

export default GlitchParams;
