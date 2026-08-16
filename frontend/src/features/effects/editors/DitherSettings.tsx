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
  | 'jarvis_judice_ninke'
  | 'stucki'
  | 'burkes'
  | 'sierra'
  | 'cmyk_halftone'
  | 'wave';

const SIMPLE_MODES: SimpleDitherMode[] = [
  'bayer_2x2',
  'bayer_4x4',
  'bayer_8x8',
  'custom_png',
  'floyd_steinberg',
  'atkinson',
  'jarvis_judice_ninke',
  'stucki',
  'burkes',
  'sierra',
  'cmyk_halftone',
  'wave',
];

interface DitherSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function modeToSimple(mode: DitherModeV2 | string | unknown): SimpleDitherMode {
  if (typeof mode === 'string') {
    if (SIMPLE_MODES.includes(mode as SimpleDitherMode)) {
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

function isGuidedMode(mode: unknown): boolean {
  return typeof mode === 'object' && mode !== null && 'guided' in mode;
}

function isMixedMode(mode: unknown): boolean {
  return typeof mode === 'object' && mode !== null && 'mixed' in mode;
}

function usesChannelLevels(mode: unknown): boolean {
  return isGuidedMode(mode) || isMixedMode(mode);
}

function paletteModeKey(mode: unknown): 'strict' | 'guided' | 'mixed' | 'simple' {
  if (isGuidedMode(mode)) return 'guided';
  if (isMixedMode(mode)) return 'mixed';
  if (mode === 'simple') return 'simple';
  return 'strict';
}

function guidedChannelLevels(mode: unknown): number | null {
  if (typeof mode !== 'object' || mode === null) return null;
  const rec = mode as { guided?: { channel_levels?: number | null }; mixed?: { channel_levels?: number | null } };
  const n = rec.guided?.channel_levels ?? rec.mixed?.channel_levels;
  return typeof n === 'number' ? n : null;
}

/**
 * Dither effect params. Palette comes from Color Lab (`lastCreatedId`), not a local selector.
 */
function DitherSettings({ params, onUpdate }: DitherSettingsProps) {
  const lastCreatedId = useAppSelector((s) => s.palettes.lastCreatedId);
  const boundPaletteId =
    lastCreatedId ?? (typeof params.palette_id === 'number' ? params.palette_id : null);
  const mode = params.mode ?? 'floyd_steinberg';
  const levels = clampParam(Number(params.levels) || 4, 2, 256);
  const thresholdScale = clampParam(Number(params.threshold_scale) || 1.0, 0.1, 4.0);
  const pixelSize = clampParam(Number(params.pixel_size) || 1, 1, 32);
  const cellSize = clampParam(Number(params.halftone_cell_size) || 8, 2, 64);
  const waveWavelength = clampParam(Number(params.wave_wavelength) || 8, 2, 256);
  const waveAmplitude = clampParam(Number(params.wave_amplitude) ?? 1, 0, 1);
  const wavePhase = Number(params.wave_phase) || 0;
  const waveAngle = Number(params.wave_angle) || 0;
  const thresholdBias = clampParam(Number(params.threshold_bias ?? 0), -0.5, 0.5);
  const patternAngle = Number(params.pattern_angle ?? 0);
  const simpleMode = modeToSimple(mode);

  const isOrderedMode = [
    'bayer_2x2',
    'bayer_4x4',
    'bayer_8x8',
    'custom_png',
    'wave',
    'cmyk_halftone',
  ].includes(simpleMode);
  const isPatternAngleMode = [
    'bayer_2x2',
    'bayer_4x4',
    'bayer_8x8',
    'custom_png',
  ].includes(simpleMode);
  const isEdMode = [
    'floyd_steinberg',
    'atkinson',
    'jarvis_judice_ninke',
    'stucki',
    'burkes',
    'sierra',
  ].includes(simpleMode);
  const serpentine = Boolean(params.serpentine);
  const ditherAlpha = params.dither_alpha !== false;
  const paletteBound = boundPaletteId != null;
  const paletteDitherMode = params.palette_dither_mode;
  const paletteMode = paletteModeKey(paletteDitherMode);
  const channelLevels = clampParam(guidedChannelLevels(paletteDitherMode) ?? 3, 2, 16);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      mode: overrides.mode ?? mode,
      levels: overrides.levels ?? levels,
      threshold_scale: overrides.threshold_scale ?? thresholdScale,
      pixel_size: overrides.pixel_size ?? pixelSize,
      color_mode: overrides.color_mode ?? (params.color_mode ?? 'rgb'),
      palette_id:
        typeof params.palette_id === 'number' ? params.palette_id : lastCreatedId,
      palette_dither_mode:
        overrides.palette_dither_mode ??
        params.palette_dither_mode ??
        'strict',
      halftone_cell_size: overrides.halftone_cell_size ?? cellSize,
      wave_wavelength: overrides.wave_wavelength ?? waveWavelength,
      wave_amplitude: overrides.wave_amplitude ?? waveAmplitude,
      wave_phase: overrides.wave_phase ?? wavePhase,
      wave_angle: overrides.wave_angle ?? waveAngle,
      threshold_bias: overrides.threshold_bias ?? thresholdBias,
      pattern_angle: overrides.pattern_angle ?? patternAngle,
      serpentine: overrides.serpentine ?? serpentine,
      dither_alpha: overrides.dither_alpha ?? ditherAlpha,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <p className={cn('effect-palette-hint')}>
        Palette is controlled in Color Lab
        {boundPaletteId != null
          ? ` (palette #${boundPaletteId}). Levels is ignored`
          : ' — extract or apply a palette first'}
        .
      </p>

      <DropdownMenu
        label="Algorithm"
        value={simpleMode}
        options={[
          { value: 'floyd_steinberg', label: 'Floyd-Steinberg' },
          { value: 'atkinson', label: 'Atkinson' },
          { value: 'jarvis_judice_ninke', label: 'Jarvis-Judice-Ninke' },
          { value: 'stucki', label: 'Stucki' },
          { value: 'burkes', label: 'Burkes' },
          { value: 'sierra', label: 'Sierra' },
          { value: 'bayer_2x2', label: 'Bayer 2×2' },
          { value: 'bayer_4x4', label: 'Bayer 4×4' },
          { value: 'bayer_8x8', label: 'Bayer 8×8' },
          { value: 'cmyk_halftone', label: 'CMYK Halftone' },
          { value: 'wave', label: 'Wave' },
          { value: 'custom_png', label: 'Custom PNG' },
        ]}
        onSelect={(v) => {
          const newMode = v as SimpleDitherMode;
          emit({ mode: simpleToMode(newMode) });
        }}
      />

      {paletteBound && (
        <>
          <DropdownMenu
            label="Palette dither"
            value={paletteMode}
            options={[
              { value: 'strict', label: 'Strict — exact palette colors' },
              { value: 'simple', label: 'Simple — sRGB Euclidean (classic)' },
              { value: 'guided', label: 'Guided — palette-derived range (richer)' },
              { value: 'mixed', label: 'Mixed — Guided then palette dither' },
            ]}
            onSelect={(v) => {
              if (v === 'guided') {
                emit({
                  palette_dither_mode: { guided: { channel_levels: channelLevels } },
                });
              } else if (v === 'mixed') {
                emit({
                  palette_dither_mode: { mixed: { channel_levels: channelLevels } },
                });
              } else if (v === 'simple') {
                emit({ palette_dither_mode: 'simple' });
              } else {
                emit({ palette_dither_mode: 'strict' });
              }
            }}
          />
          {usesChannelLevels(paletteDitherMode) && (
            <Slider
              label="Levels per channel"
              value={channelLevels}
              min={2}
              max={16}
              step={1}
              decimals={0}
              onChange={(v) =>
                emit({
                  palette_dither_mode:
                    paletteMode === 'mixed'
                      ? { mixed: { channel_levels: clampParam(Math.round(v), 2, 16) } }
                      : { guided: { channel_levels: clampParam(Math.round(v), 2, 16) } },
                })
              }
            />
          )}
        </>
      )}

      <Slider
        label="Pixel Size"
        value={pixelSize}
        min={1}
        max={32}
        step={1}
        decimals={0}
        onChange={(v) => emit({ pixel_size: clampParam(Math.round(v), 1, 32) })}
      />

      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={ditherAlpha}
          onChange={(e) => emit({ dither_alpha: e.target.checked })}
        />
        Pixelate Alpha
      </label>

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

      {isEdMode && (
        <label className={cn('param-checkbox-row')}>
          <input
            type="checkbox"
            checked={serpentine}
            onChange={(e) => emit({ serpentine: e.target.checked })}
          />
          Serpentine
        </label>
      )}

      {isOrderedMode && (
        <Slider
          label="Threshold Bias"
          value={thresholdBias}
          min={-0.5}
          max={0.5}
          step={0.01}
          decimals={2}
          onChange={(v) => emit({ threshold_bias: clampParam(v, -0.5, 0.5) })}
        />
      )}

      {isPatternAngleMode && (
        <Slider
          label="Pattern Angle"
          value={patternAngle}
          min={0}
          max={360}
          step={1}
          decimals={0}
          onChange={(v) => emit({ pattern_angle: v })}
        />
      )}

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
