import type { DitherModeV2 } from '../../../types';
import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import DropdownMenu from '../../../components/common/DropdownMenu';
import { useAppSelector } from '../../../app/hooks';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

type SimpleDitherMode =
  | 'bayer_2x2'
  | 'bayer_4x4'
  | 'bayer_8x8'
  | 'custom_png'
  | 'floyd_steinberg'
  | 'atkinson'
  | 'cmyk_halftone'
  | 'wave';

interface DitherSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function modeToSimple(mode: DitherModeV2 | string | unknown): SimpleDitherMode {
  if (typeof mode === 'string') {
    if (
      [
        'bayer_2x2',
        'bayer_4x4',
        'bayer_8x8',
        'custom_png',
        'floyd_steinberg',
        'atkinson',
        'cmyk_halftone',
        'wave',
      ].includes(mode)
    ) {
      return mode as SimpleDitherMode;
    }
    return 'floyd_steinberg';
  }
  if (typeof mode === 'object' && mode !== null && 'custom_png' in mode) return 'custom_png';
  return 'floyd_steinberg';
}

function simpleToMode(simple: SimpleDitherMode): DitherModeV2 | string {
  if (simple === 'custom_png') return { custom_png: { path: '' } } as unknown as DitherModeV2;
  return simple;
}

/**
 * Dither effect params. Palette comes from Color Lab (`lastCreatedId`), not a local selector.
 */
function DitherSettings({ params, onUpdate }: DitherSettingsProps) {
  const lastCreatedId = useAppSelector((s) => s.palettes.lastCreatedId);
  const mode = params.mode ?? 'floyd_steinberg';
  const levels = clampParam(Number(params.levels) || 4, 2, 256);
  const thresholdScale = clampParam(Number(params.threshold_scale) || 1.0, 0.1, 4.0);
  const pixelSize = clampParam(Number(params.pixel_size) || 1, 1, 32);
  const cellSize = clampParam(Number(params.halftone_cell_size) || 8, 2, 64);
  const waveWavelength = clampParam(Number(params.wave_wavelength) || 8, 2, 256);
  const waveAmplitude = clampParam(Number(params.wave_amplitude) ?? 1, 0, 1);
  const wavePhase = Number(params.wave_phase) || 0;
  const waveAngle = Number(params.wave_angle) || 0;
  const simpleMode = modeToSimple(mode);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      mode: overrides.mode ?? mode,
      levels: overrides.levels ?? levels,
      threshold_scale: overrides.threshold_scale ?? thresholdScale,
      pixel_size: overrides.pixel_size ?? pixelSize,
      color_mode: overrides.color_mode ?? (params.color_mode ?? 'rgb'),
      palette_id: lastCreatedId,
      halftone_cell_size: overrides.halftone_cell_size ?? cellSize,
      wave_wavelength: overrides.wave_wavelength ?? waveWavelength,
      wave_amplitude: overrides.wave_amplitude ?? waveAmplitude,
      wave_phase: overrides.wave_phase ?? wavePhase,
      wave_angle: overrides.wave_angle ?? waveAngle,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <p className={cn('effect-palette-hint')}>
        Palette is controlled in Color Lab
        {lastCreatedId != null ? ` (palette #${lastCreatedId})` : ' — extract or apply a palette first'}.
      </p>

      <DropdownMenu
        label="Algorithm"
        value={simpleMode}
        options={[
          { value: 'floyd_steinberg', label: 'Floyd-Steinberg' },
          { value: 'bayer_2x2', label: 'Bayer 2×2' },
          { value: 'bayer_4x4', label: 'Bayer 4×4' },
          { value: 'bayer_8x8', label: 'Bayer 8×8' },
          { value: 'cmyk_halftone', label: 'CMYK Halftone' },
          { value: 'wave', label: 'Wave' },
          { value: 'custom_png', label: 'Custom PNG' },
          { value: 'atkinson', label: 'Atkinson' },
        ]}
        onSelect={(v) => {
          const newMode = v as SimpleDitherMode;
          emit({ mode: simpleToMode(newMode) });
        }}
      />

      <Slider
        label="Pixel Size"
        value={pixelSize}
        min={1}
        max={32}
        step={1}
        decimals={0}
        onChange={(v) => emit({ pixel_size: clampParam(Math.round(v), 1, 32) })}
      />

      <Slider
        label="Threshold Scale"
        value={thresholdScale}
        min={0.1}
        max={4.0}
        step={0.1}
        decimals={1}
        onChange={(v) => emit({ threshold_scale: clampParam(v, 0.1, 4.0) })}
      />

      <Slider
        label="Levels"
        value={levels}
        min={2}
        max={256}
        step={1}
        decimals={0}
        onChange={(v) => emit({ levels: clampParam(Math.round(v), 2, 256) })}
      />

      {simpleMode === 'cmyk_halftone' && (
        <Slider
          label="Halftone Cell Size"
          value={cellSize}
          min={2}
          max={64}
          step={1}
          decimals={0}
          onChange={(v) => emit({ halftone_cell_size: clampParam(Math.round(v), 2, 64) })}
        />
      )}

      {simpleMode === 'wave' && (
        <>
          <Slider
            label="Wavelength"
            value={waveWavelength}
            min={2}
            max={256}
            step={1}
            decimals={0}
            onChange={(v) => emit({ wave_wavelength: clampParam(Math.round(v), 2, 256) })}
          />
          <Slider
            label="Amplitude"
            value={waveAmplitude}
            min={0}
            max={1}
            step={0.05}
            decimals={2}
            onChange={(v) => emit({ wave_amplitude: clampParam(v, 0, 1) })}
          />
          <Slider
            label="Phase"
            value={wavePhase}
            min={-6.28}
            max={6.28}
            step={0.1}
            decimals={2}
            onChange={(v) => emit({ wave_phase: v })}
          />
          <Slider
            label="Angle"
            value={waveAngle}
            min={0}
            max={180}
            step={1}
            decimals={0}
            onChange={(v) => emit({ wave_angle: v })}
          />
        </>
      )}
    </div>
  );
}

export default DitherSettings;
