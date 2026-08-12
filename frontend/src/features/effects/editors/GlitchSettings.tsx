import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import DropdownMenu from '../../../components/common/DropdownMenu';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import inputStyles from '../../../shared/ui/ParamInput.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind({ ...panelStyles, ...paramStyles, ...inputStyles, ...sliderStyles });

interface GlitchSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function GlitchSettings({ params, onUpdate }: GlitchSettingsProps) {
  const glitchType = (params.glitch_type as string) || 'RGBShift';
  const intensity = clampParam(Number(params.intensity) || 0.5, 0.0, 1.0);
  const seed = clampParam(Number(params.seed) || 0, 0, 99999);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      glitch_type: overrides.glitch_type ?? glitchType,
      intensity: overrides.intensity ?? intensity,
      seed: overrides.seed ?? seed,
    });
  };

  return (
    <div className={cn("effect-settings-content")}>
      {/* Glitch Type dropdown */}
      <DropdownMenu
        label="Glitch Type"
        value={glitchType}
        options={[
          { value: 'RGBShift', label: 'RGB Shift' },
          { value: 'BlockDisplace', label: 'Block Displace' },
        ]}
        onSelect={(v) => emit({ glitch_type: v })}
      />

      {/* Intensity slider */}
      <Slider
        label="Intensity"
        value={intensity}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ intensity: clampParam(v, 0.0, 1.0) })}
      />

      {/* Seed number input */}
      <div className={cn("param-group")}>
        <label className={cn("slider-label")}>Seed</label>
        <input
          type="number"
          className={cn("param-input")}
          min={0}
          max={99999}
          value={seed}
          onChange={(e) => {
            const val = clampParam(Math.round(Number(e.target.value) || 0), 0, 99999);
            emit({ seed: val });
          }}
        />
      </div>
    </div>
  );
}

export default GlitchSettings;
