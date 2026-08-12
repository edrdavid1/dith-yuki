import { useState } from 'react';
import DropdownMenu from '../common/DropdownMenu';
import type { CurveChannel } from '../../types';
import curveStyles from '../../features/effects/editors/CurvesSettings.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import buttonStyles from '../../shared/ui/FilterButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...curveStyles, ...paramStyles, ...sliderStyles, ...buttonStyles });

interface CurvesParamsProps {
  curve: [number, number][];
  channel: CurveChannel;
  onChange: (params: Record<string, unknown>) => void;
}

function CurvesParams({ curve, channel, onChange }: CurvesParamsProps) {
  const [newX, setNewX] = useState(0.5);
  const [newY, setNewY] = useState(0.5);

  const handleChannelChange = (newChannel: string) => {
    onChange({ curve, channel: newChannel });
  };

  const handleAddPoint = () => {
    const clamped = [Math.max(0, Math.min(1, newX)), Math.max(0, Math.min(1, newY))] as [number, number];
    const updated = [...curve, clamped].sort((a, b) => a[0] - b[0]);
    onChange({ curve: updated, channel });
  };

  const handleRemovePoint = (index: number) => {
    // Don't allow removing first or last point
    if (index === 0 || index === curve.length - 1) return;
    const updated = curve.filter((_, i) => i !== index);
    onChange({ curve: updated, channel });
  };

  const handlePointChange = (index: number, axis: 0 | 1, value: number) => {
    const clamped = Math.max(0, Math.min(1, value));
    const updated = [...curve];
    updated[index] = [...updated[index]] as [number, number];
    updated[index][axis] = clamped;
    // Re-sort by x if x changed
    if (axis === 0) {
      updated.sort((a, b) => a[0] - b[0]);
    }
    onChange({ curve: updated, channel });
  };

  return (
    <div className={cn("filter-params")}>
      <DropdownMenu
        label="Channel"
        value={channel}
        options={[
          { value: 'All', label: 'All' },
          { value: 'Red', label: 'Red' },
          { value: 'Green', label: 'Green' },
          { value: 'Blue', label: 'Blue' },
          { value: 'Luminance', label: 'Luminance' },
        ]}
        onSelect={(v) => handleChannelChange(v)}
      />

      <div className={cn("param-group")}>
        <label className={cn("slider-label")}>Control Points</label>
        <div className={cn("curve-points")}>
          {curve.map(([x, y], i) => (
            <div key={i} className={cn("curve-point-row")}>
              <input type="number" className={cn("curve-input")} min={0} max={1} step={0.05} value={x.toFixed(2)} onChange={(e) => handlePointChange(i, 0, parseFloat(e.target.value) || 0)} />
              <span className={cn("curve-arrow")}>→</span>
              <input type="number" className={cn("curve-input")} min={0} max={1} step={0.05} value={y.toFixed(2)} onChange={(e) => handlePointChange(i, 1, parseFloat(e.target.value) || 0)} />
              {i > 0 && i < curve.length - 1 && (
                <button className={cn("curve-remove-btn")} onClick={() => handleRemovePoint(i)}>×</button>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className={cn("curve-add-row")}>
        <input type="number" className={cn("curve-input")} min={0} max={1} step={0.05} value={newX.toFixed(2)} onChange={(e) => setNewX(parseFloat(e.target.value) || 0)} />
        <span className={cn("curve-arrow")}>→</span>
        <input type="number" className={cn("curve-input")} min={0} max={1} step={0.05} value={newY.toFixed(2)} onChange={(e) => setNewY(parseFloat(e.target.value) || 0)} />
        <button className={cn("filter-add-btn")} onClick={handleAddPoint}>+</button>
      </div>
    </div>
  );
}

export default CurvesParams;
