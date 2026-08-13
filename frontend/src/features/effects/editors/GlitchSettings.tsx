import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import NumberInput from '../../../components/common/NumberInput';
import DropdownMenu from '../../../components/common/DropdownMenu';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

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

      <NumberInput
        label="Seed"
        value={seed}
        min={0}
        max={99999}
        step={1}
        decimals={0}
        onChange={(v) => emit({ seed: clampParam(Math.round(v), 0, 99999) })}
      />
    </div>
  );
}

export default GlitchSettings;
