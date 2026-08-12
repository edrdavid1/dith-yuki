import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface CrtSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function CrtSettings({ params, onUpdate }: CrtSettingsProps) {
  const period = clampParam(Number(params.period) || 2, 2, 8);
  const strength = clampParam(Number(params.strength) || 0.5, 0, 1);
  const maskStrength = clampParam(Number(params.mask_strength) || 0, 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      period: overrides.period ?? period,
      strength: overrides.strength ?? strength,
      mask_strength: overrides.mask_strength ?? maskStrength,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Period"
        value={period}
        min={2}
        max={8}
        step={1}
        decimals={0}
        onChange={(v) => emit({ period: clampParam(Math.round(v), 2, 8) })}
      />
      <Slider
        label="Strength"
        value={strength}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ strength: clampParam(v, 0, 1) })}
      />
      <Slider
        label="RGB Mask"
        value={maskStrength}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ mask_strength: clampParam(v, 0, 1) })}
      />
    </div>
  );
}

export default CrtSettings;
