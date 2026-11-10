import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface BlurNoiseSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) ? v : fallback;
}

function BlurNoiseSettings({ params, onUpdate }: BlurNoiseSettingsProps) {
  const blur = clampParam(num(params, 'blur', 0), 0, 2);
  const noise = clampParam(num(params, 'noise', 0), 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      blur: overrides.blur ?? blur,
      noise: overrides.noise ?? noise,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Blur"
        value={blur}
        min={0}
        max={2}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ blur: clampParam(v, 0, 2) })}
      />
      <Slider
        label="Noise"
        value={noise}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ noise: clampParam(v, 0, 1) })}
      />
    </div>
  );
}

export default BlurNoiseSettings;
