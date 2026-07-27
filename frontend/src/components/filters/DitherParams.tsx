import Slider from '../common/Slider';
import type { DitherAlgorithm } from '../../types';

interface DitherParamsProps {
  algorithm: DitherAlgorithm;
  colorDepth: number;
  onChange: (params: Record<string, unknown>) => void;
}

function DitherParams({ algorithm, colorDepth, onChange }: DitherParamsProps) {
  return (
    <div className="filter-params">
      <div className="param-group">
        <label className="slider-label">Algorithm</label>
        <select
          className="param-select"
          value={algorithm}
          onChange={(e) => onChange({ algorithm: e.target.value, color_depth: colorDepth })}
        >
          <option value="FloydSteinberg">Floyd-Steinberg</option>
          <option value="Ordered">Ordered (Bayer)</option>
          <option value="Threshold">Threshold</option>
        </select>
      </div>
      <Slider
        label="Color Depth (bits)"
        value={colorDepth}
        min={1}
        max={8}
        step={1}
        decimals={0}
        onChange={(val) => {
          const clamped = Math.round(Math.max(1, Math.min(8, val)));
          onChange({ algorithm, color_depth: clamped });
        }}
      />
    </div>
  );
}

export default DitherParams;
