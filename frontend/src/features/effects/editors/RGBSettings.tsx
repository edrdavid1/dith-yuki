import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind(panelStyles);

interface RGBSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function RGBSettings({ params, onUpdate }: RGBSettingsProps) {
  const inputBlack = clampParam(Number(params.input_black) || 0, 0.0, 1.0);
  const inputWhite = clampParam(Number(params.input_white) ?? 1, 0.0, 1.0);
  const gamma = clampParam(Number(params.gamma) || 1, 0.1, 10.0);
  const outputBlack = clampParam(Number(params.output_black) || 0, 0.0, 1.0);
  const outputWhite = clampParam(Number(params.output_white) ?? 1, 0.0, 1.0);

  const emit = (key: string, value: number) => {
    onUpdate({
      input_black: inputBlack,
      input_white: inputWhite,
      gamma,
      output_black: outputBlack,
      output_white: outputWhite,
      [key]: value,
    });
  };

  return (
    <div className={cn("effect-settings-content")}>
      <Slider
        label="Input Black"
        value={inputBlack}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit('input_black', clampParam(v, 0.0, 1.0))}
      />
      <Slider
        label="Input White"
        value={inputWhite}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit('input_white', clampParam(v, 0.0, 1.0))}
      />
      <Slider
        label="Gamma"
        value={gamma}
        min={0.1}
        max={10}
        step={0.1}
        decimals={1}
        onChange={(v) => emit('gamma', clampParam(v, 0.1, 10.0))}
      />
      <Slider
        label="Output Black"
        value={outputBlack}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit('output_black', clampParam(v, 0.0, 1.0))}
      />
      <Slider
        label="Output White"
        value={outputWhite}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit('output_white', clampParam(v, 0.0, 1.0))}
      />
    </div>
  );
}

export default RGBSettings;
