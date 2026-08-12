import { clampParam } from '../../../types/effects';
import DropdownMenu from '../../../components/common/DropdownMenu';
import styles from './CurvesSettings.module.css';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import buttonStyles from '../../../shared/ui/FilterButtons.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind({ ...styles, ...panelStyles, ...paramStyles, ...sliderStyles, ...buttonStyles });

interface CurvesSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function CurvesSettings({ params, onUpdate }: CurvesSettingsProps) {
  const channel = (params.channel as string) || 'All';
  const curve = (params.curve as [number, number][] | undefined) ?? [[0, 0], [1, 1]];

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      curve: overrides.curve ?? curve,
      channel: overrides.channel ?? channel,
    });
  };

  const handlePointChange = (index: number, axis: 'x' | 'y', value: number) => {
    const newCurve = [...curve];
    const point = [...newCurve[index]] as [number, number];
    point[axis === 'x' ? 0 : 1] = clampParam(value, 0, 1);
    newCurve[index] = point;
    emit({ curve: newCurve });
  };

  const handleAddPoint = () => {
    const newCurve = [...curve, [0.5, 0.5] as [number, number]];
    newCurve.sort((a, b) => a[0] - b[0]);
    emit({ curve: newCurve });
  };

  const handleRemovePoint = (index: number) => {
    if (curve.length <= 2) return; // need at least 2 points
    const newCurve = curve.filter((_, i) => i !== index);
    emit({ curve: newCurve });
  };

  return (
    <div className={cn("effect-settings-content")}>
      {/* Channel dropdown */}
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
        onSelect={(v) => emit({ channel: v })}
      />

      {/* Curve points editor */}
      <div className={cn("param-group")}>
        <label className={cn("slider-label")}>Curve Points</label>
        <div className={cn("curve-points")}>
          {curve.map((point, idx) => (
            <div key={idx} className={cn("curve-point-row")}>
              <input
                type="number"
                className={cn("curve-input")}
                value={point[0].toFixed(2)}
                min={0}
                max={1}
                step={0.05}
                onChange={(e) => handlePointChange(idx, 'x', parseFloat(e.target.value) || 0)}
              />
              <span className={cn("curve-arrow")}>→</span>
              <input
                type="number"
                className={cn("curve-input")}
                value={point[1].toFixed(2)}
                min={0}
                max={1}
                step={0.05}
                onChange={(e) => handlePointChange(idx, 'y', parseFloat(e.target.value) || 0)}
              />
              {curve.length > 2 && (
                <button className={cn("curve-remove-btn")} onClick={() => handleRemovePoint(idx)}>×</button>
              )}
            </div>
          ))}
          <div className={cn("curve-add-row")}>
            <button className={cn("filter-add-btn")} onClick={handleAddPoint}>+ Add Point</button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default CurvesSettings;
