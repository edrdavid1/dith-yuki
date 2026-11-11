import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface DetailSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) ? v : fallback;
}

function DetailSettings({ params, onUpdate }: DetailSettingsProps) {
  const clarity = clampParam(num(params, 'clarity', 0), -1, 1);
  const sharpness = clampParam(num(params, 'sharpness', 0), 0, 2);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      clarity: overrides.clarity ?? clarity,
      sharpness: overrides.sharpness ?? sharpness,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
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

export default DetailSettings;
