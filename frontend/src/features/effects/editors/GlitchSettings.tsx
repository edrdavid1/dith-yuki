import { clampParam } from '../../../types/effects';
import type { GlitchFamilyAlgorithmId } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import NumberInput from '../../../components/common/NumberInput';
import DropdownMenu from '../../../components/common/DropdownMenu';
import AlgorithmSettingsPanel from '../AlgorithmSettingsPanel';
import type { AlgorithmInfo } from '../../../shared/ipc/registry';
import { useAppSelector } from '../../../app/hooks';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import buttonStyles from '../../../shared/ui/FilterButtons.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles, ...buttonStyles });

const SEED_MIN = 0;
const SEED_MAX = 99999;

function randomSeed(): number {
  return Math.floor(Math.random() * (SEED_MAX + 1));
}

function SeedField({
  value,
  onChange,
}: {
  value: number;
  onChange: (seed: number) => void;
}) {
  return (
    <div className={cn('threshold-map-row')}>
      <span className={cn('threshold-map-label')}>Seed</span>
      <div className={cn('threshold-map-controls')}>
        <NumberInput
          label="Seed value"
          value={value}
          min={SEED_MIN}
          max={SEED_MAX}
          step={1}
          decimals={0}
          compact
          onChange={(v) => onChange(clampParam(Math.round(v), SEED_MIN, SEED_MAX))}
        />
        <button
          type="button"
          className={cn('filter-add-btn')}
          aria-label="Randomize seed"
          title="Randomize seed"
          onClick={() => onChange(randomSeed())}
        >
          Random
        </button>
      </div>
    </div>
  );
}

interface GlitchSubOp {
  enabled: boolean;
  amount: number;
}

interface GlitchSettingsProps {
  params: Record<string, unknown>;
  /** Active registry id (`glitch` | `modulated_diffuse_y` | …). */
  algorithmId: string;
  /** Glitch-category algorithms from the catalog (drives the type dropdown). */
  glitchAlgorithms: AlgorithmInfo[];
  onUpdate: (params: Record<string, unknown>) => void;
  /** Swap filter kind/params on the same layer (not a new layer). */
  onChangeType?: (algorithmId: string) => void;
}

function readSub(params: Record<string, unknown>, key: string): GlitchSubOp {
  const raw = params[key];
  if (raw && typeof raw === 'object') {
    const o = raw as Record<string, unknown>;
    return {
      enabled: Boolean(o.enabled),
      amount: Number(o.amount) || 0,
    };
  }
  return { enabled: false, amount: 0 };
}

function resolveTypeId(algorithmId: string, params: Record<string, unknown>): string {
  if (algorithmId && algorithmId !== 'glitch') return algorithmId;
  if (
    params.mode === 'modulated_diffuse_y' ||
    params.mode === 'modulated_diffuse_x' ||
    params.mode === 'glitch_dither' ||
    params.mode === 'contrast_aware_y' ||
    params.mode === 'contrast_aware_x' ||
    params.mode === 'smooth_diffuse' ||
    params.mode === 'wave_modulation_y' ||
    params.mode === 'wave_modulation_x'
  ) {
    return String(params.mode);
  }
  return 'glitch';
}

function GlitchSettings({
  params,
  algorithmId,
  glitchAlgorithms,
  onUpdate,
  onChangeType,
}: GlitchSettingsProps) {
  const lastCreatedId = useAppSelector((s) => s.palettes.lastCreatedId);
  const toneStepsSelected = lastCreatedId == null;
  const typeId = resolveTypeId(algorithmId, params);

  const typeOptions = (() => {
    const fromCatalog = glitchAlgorithms.map((a) => ({
      value: a.id,
      label: a.display_name,
    }));
    if (fromCatalog.length > 0) return fromCatalog;
    return [
      { value: 'glitch', label: 'Block / Channel Glitch' },
      { value: 'modulated_diffuse_y', label: 'Modulated Diffuse Y' },
      { value: 'modulated_diffuse_x', label: 'Modulated Diffuse X' },
      { value: 'glitch_dither', label: 'Corrupt Dither' },
      { value: 'contrast_aware_y', label: 'Contrast Aware Y' },
      { value: 'contrast_aware_x', label: 'Contrast Aware X' },
      { value: 'smooth_diffuse', label: 'Smooth Diffuse' },
      { value: 'wave_modulation_y', label: 'Wave Modulation Y' },
      { value: 'wave_modulation_x', label: 'Wave Modulation X' },
    ];
  })();

  const enabled = params.enabled !== false;
  const seed = clampParam(Number(params.seed ?? 42), 0, 99999);
  const blockSize = String(params.block_size ?? 8);
  const groupSize = String(params.group_size ?? 4);
  const scanlineSegment = String(params.scanline_segment ?? 64);
  const blockShift = readSub(params, 'block_shift');
  const blockScramble = readSub(params, 'block_scramble');
  const channelSwap = readSub(params, 'channel_swap');
  const scanline = readSub(params, 'scanline');
  const interlace = readSub(params, 'interlace');

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      enabled: overrides.enabled ?? enabled,
      seed: overrides.seed ?? seed,
      block_size: Number(overrides.block_size ?? blockSize),
      group_size: Number(overrides.group_size ?? groupSize),
      scanline_segment: Number(overrides.scanline_segment ?? scanlineSegment),
      block_shift: overrides.block_shift ?? blockShift,
      block_scramble: overrides.block_scramble ?? blockScramble,
      channel_swap: overrides.channel_swap ?? channelSwap,
      scanline: overrides.scanline ?? scanline,
      interlace: overrides.interlace ?? interlace,
    });
  };

  const subSlider = (
    label: string,
    key: string,
    sub: GlitchSubOp,
    max: number,
  ) => (
    <div key={key}>
      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={sub.enabled}
          onChange={(e) =>
            emit({
              [key]: { enabled: e.target.checked, amount: sub.amount },
            })
          }
        />
        <span>{label}</span>
      </label>
      <Slider
        label={`${label} amount`}
        value={sub.amount}
        min={0}
        max={max}
        step={1}
        decimals={0}
        onChange={(v) =>
          emit({
            [key]: {
              enabled: sub.enabled,
              amount: clampParam(v, 0, max),
            },
          })
        }
      />
    </div>
  );

  const typePicker = (
    <DropdownMenu
      label="Glitch type"
      value={typeId}
      options={typeOptions}
      onSelect={(v) => {
        if (v !== typeId) onChangeType?.(v);
      }}
      ariaLabel="Glitch type"
    />
  );

  if (typeId !== 'glitch') {
    const paletteId =
      lastCreatedId ??
      (typeof params.palette_id === 'number' ? params.palette_id : null);
    const levels = clampParam(Number(params.levels ?? 4), 2, 256);
    const glitchSeed = clampParam(
      Number(params.glitch_seed ?? 42),
      SEED_MIN,
      SEED_MAX,
    );
    const hasGlitchSeed = typeId === 'glitch_dither';

    return (
      <div className={cn('effect-settings-content')}>
        {typePicker}

        <p className={cn('effect-palette-hint')}>
          {toneStepsSelected
            ? 'Raw tone selected in Color Lab — the Tone Steps slider below sets how many shades each channel can use.'
            : `Palette #${paletteId} (Color Lab) chooses the output colours.`}
        </p>

        {toneStepsSelected && (
          <>
            <Slider
              label="Tone Steps"
              value={levels}
              min={2}
              max={256}
              step={1}
              decimals={0}
              onChange={(v) =>
                onUpdate({ levels: clampParam(Math.round(v), 2, 256) })
              }
            />
            <p className={cn('effect-palette-hint')}>
              Each RGB channel snaps to this many evenly spaced values before
              error diffusion fills the gaps as a pattern. 2 = stark
              black/white per channel; higher = smoother, less posterized.
            </p>
          </>
        )}

        {hasGlitchSeed && (
          <SeedField
            value={glitchSeed}
            onChange={(v) => onUpdate({ glitch_seed: v })}
          />
        )}

        <AlgorithmSettingsPanel
          algorithmId={typeId as GlitchFamilyAlgorithmId}
          values={params}
          onChange={onUpdate}
          omitKeys={hasGlitchSeed ? ['levels', 'glitch_seed'] : ['levels']}
        />
      </div>
    );
  }

  return (
    <div className={cn('effect-settings-content')}>
      {typePicker}

      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => emit({ enabled: e.target.checked })}
        />
        <span>Enabled</span>
      </label>

      <DropdownMenu
        label="Block Size"
        value={blockSize}
        options={[
          { value: '8', label: '8' },
          { value: '16', label: '16' },
          { value: '32', label: '32' },
        ]}
        onSelect={(v) => emit({ block_size: Number(v) })}
      />

      <DropdownMenu
        label="Group Size"
        value={groupSize}
        options={[
          { value: '2', label: '2' },
          { value: '4', label: '4' },
          { value: '8', label: '8' },
        ]}
        onSelect={(v) => emit({ group_size: Number(v) })}
      />

      <DropdownMenu
        label="Scanline Segment"
        value={scanlineSegment}
        options={[
          { value: '64', label: '64' },
          { value: '128', label: '128' },
          { value: '256', label: '256' },
        ]}
        onSelect={(v) => emit({ scanline_segment: Number(v) })}
      />

      {subSlider('Block Shift', 'block_shift', blockShift, 20)}
      {subSlider('Block Scramble', 'block_scramble', blockScramble, 20)}
      {subSlider('Channel Swap', 'channel_swap', channelSwap, 100)}
      {subSlider('Scanline', 'scanline', scanline, 100)}
      {subSlider('Interlace', 'interlace', interlace, 100)}

      <SeedField
        value={seed}
        onChange={(v) => emit({ seed: v })}
      />
    </div>
  );
}

export default GlitchSettings;
