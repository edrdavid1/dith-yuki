import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface GlowSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function GlowSettings({ params, onUpdate }: GlowSettingsProps) {
  const radius = clampParam(Number(params.radius) || 2, 0.5, 2);
  const intensity = clampParam(Number(params.intensity) || 1, 0, 4);
  const threshold = clampParam(Number(params.threshold) || 0, 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      radius: overrides.radius ?? radius,
      intensity: overrides.intensity ?? intensity,
      threshold: overrides.threshold ?? threshold,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Radius"
        value={radius}
        min={0.5}
        max={2}
        step={0.5}
        decimals={1}
        onChange={(v) => emit({ radius: clampParam(v, 0.5, 2) })}
      />
      <Slider
        label="Intensity"
        value={intensity}
        min={0}
        max={4}
        step={0.1}
        decimals={1}
        onChange={(v) => emit({ intensity: clampParam(v, 0, 4) })}
      />
      <Slider
        label="Threshold"
        value={threshold}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ threshold: clampParam(v, 0, 1) })}
      />
    </div>
  );
}

export default GlowSettings;
