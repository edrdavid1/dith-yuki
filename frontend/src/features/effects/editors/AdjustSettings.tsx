import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface AdjustSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) ? v : fallback;
}

function AdjustSettings({ params, onUpdate }: AdjustSettingsProps) {
  const contrast = clampParam(num(params, 'contrast', 0), -1, 1);
  const brightness = clampParam(num(params, 'brightness', 0), -1, 1);
  const saturation = clampParam(num(params, 'saturation', 0), -1, 1);
  const blur = clampParam(num(params, 'blur', 0), 0, 2);
  const sharpness = clampParam(num(params, 'sharpness', 0), 0, 2);
  const noise = clampParam(num(params, 'noise', 0), 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      contrast: overrides.contrast ?? contrast,
      brightness: overrides.brightness ?? brightness,
      saturation: overrides.saturation ?? saturation,
      blur: overrides.blur ?? blur,
      sharpness: overrides.sharpness ?? sharpness,
      noise: overrides.noise ?? noise,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Contrast"
        value={contrast}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ contrast: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Brightness"
        value={brightness}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ brightness: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Saturation"
        value={saturation}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ saturation: clampParam(v, -1, 1) })}
      />
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
        label="Sharpness"
        value={sharpness}
        min={0}
        max={2}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ sharpness: clampParam(v, 0, 2) })}
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

export default AdjustSettings;
