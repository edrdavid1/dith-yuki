import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import { unwrapFilterParams } from '../../../shared/unwrapFilterParams';
import panelStyles from '../EffectSettingsPanel.module.css';
import rgbStyles from './RGBSettings.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...rgbStyles });

interface RGBSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) ? v : fallback;
}

function flag(params: Record<string, unknown>, key: string): boolean {
  return params[key] !== false;
}

function RGBSettings({ params, onUpdate }: RGBSettingsProps) {
  const p = unwrapFilterParams(params);
  const inputBlack = clampParam(num(p, 'input_black', 0), 0.0, 1.0);
  const inputWhite = clampParam(num(p, 'input_white', 1), 0.0, 1.0);
  const gamma = clampParam(num(p, 'gamma', 1), 0.1, 10.0);
  const outputBlack = clampParam(num(p, 'output_black', 0), 0.0, 1.0);
  const outputWhite = clampParam(num(p, 'output_white', 1), 0.0, 1.0);
  const channelR = flag(p, 'channel_r');
  const channelG = flag(p, 'channel_g');
  const channelB = flag(p, 'channel_b');

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      input_black: inputBlack,
      input_white: inputWhite,
      gamma,
      output_black: outputBlack,
      output_white: outputWhite,
      channel_r: channelR,
      channel_g: channelG,
      channel_b: channelB,
      ...overrides,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <div className={cn('channel-toggles')} role="group" aria-label="RGB channels">
        <button
          type="button"
          className={cn('channel-btn', 'r', channelR ? 'on' : 'off')}
          aria-pressed={channelR}
          aria-label="Red channel"
          onClick={() => emit({ channel_r: !channelR })}
        >
          R
        </button>
        <button
          type="button"
          className={cn('channel-btn', 'g', channelG ? 'on' : 'off')}
          aria-pressed={channelG}
          aria-label="Green channel"
          onClick={() => emit({ channel_g: !channelG })}
        >
          G
        </button>
        <button
          type="button"
          className={cn('channel-btn', 'b', channelB ? 'on' : 'off')}
          aria-pressed={channelB}
          aria-label="Blue channel"
          onClick={() => emit({ channel_b: !channelB })}
        >
          B
        </button>
      </div>

      <Slider
        label="Input Black"
        value={inputBlack}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ input_black: clampParam(v, 0.0, 1.0) })}
      />
      <Slider
        label="Input White"
        value={inputWhite}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ input_white: clampParam(v, 0.0, 1.0) })}
      />
      <Slider
        label="Gamma"
        value={gamma}
        min={0.1}
        max={10}
        step={0.1}
        decimals={1}
        onChange={(v) => emit({ gamma: clampParam(v, 0.1, 10.0) })}
      />
      <Slider
        label="Output Black"
        value={outputBlack}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ output_black: clampParam(v, 0.0, 1.0) })}
      />
      <Slider
        label="Output White"
        value={outputWhite}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ output_white: clampParam(v, 0.0, 1.0) })}
      />
    </div>
  );
}

export default RGBSettings;
