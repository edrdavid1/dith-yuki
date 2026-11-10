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
  const exposure = clampParam(num(params, 'exposure', 0), -5, 5);
  const contrast = clampParam(num(params, 'contrast', 0), -1, 1);
  const brightness = clampParam(num(params, 'brightness', 0), -1, 1);
  const shadows = clampParam(num(params, 'shadows', 0), -1, 1);
  const highlights = clampParam(num(params, 'highlights', 0), -1, 1);
  const temperature = clampParam(num(params, 'temperature', 0), -1, 1);
  const tint = clampParam(num(params, 'tint', 0), -1, 1);
  const saturation = clampParam(num(params, 'saturation', 0), -1, 1);
  const vibrance = clampParam(num(params, 'vibrance', 0), -1, 1);
  const hue = clampParam(num(params, 'hue', 0), -180, 180);
  const clarity = clampParam(num(params, 'clarity', 0), -1, 1);
  const sharpness = clampParam(num(params, 'sharpness', 0), 0, 2);
  // Legacy blur/noise may still live on Adjust from old docs — preserve on update.
  const blur = clampParam(num(params, 'blur', 0), 0, 2);
  const noise = clampParam(num(params, 'noise', 0), 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      exposure: overrides.exposure ?? exposure,
      contrast: overrides.contrast ?? contrast,
      brightness: overrides.brightness ?? brightness,
      shadows: overrides.shadows ?? shadows,
      highlights: overrides.highlights ?? highlights,
      temperature: overrides.temperature ?? temperature,
      tint: overrides.tint ?? tint,
      saturation: overrides.saturation ?? saturation,
      vibrance: overrides.vibrance ?? vibrance,
      hue: overrides.hue ?? hue,
      clarity: overrides.clarity ?? clarity,
      sharpness: overrides.sharpness ?? sharpness,
      blur,
      noise,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Exposure"
        value={exposure}
        min={-5}
        max={5}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ exposure: clampParam(v, -5, 5) })}
      />
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
        label="Shadows"
        value={shadows}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ shadows: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Highlights"
        value={highlights}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ highlights: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Temperature"
        value={temperature}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ temperature: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Tint"
        value={tint}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ tint: clampParam(v, -1, 1) })}
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
        label="Vibrance"
        value={vibrance}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ vibrance: clampParam(v, -1, 1) })}
      />
      <Slider
        label="Hue"
        value={hue}
        min={-180}
        max={180}
        step={1}
        decimals={0}
        onChange={(v) => emit({ hue: clampParam(v, -180, 180) })}
      />
      <Slider
        label="Clarity"
        value={clarity}
        min={-1}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ clarity: clampParam(v, -1, 1) })}
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
    </div>
  );
}

export default AdjustSettings;
