import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import DropdownMenu from '../../../components/common/DropdownMenu';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface CrtSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

const ORIENT_OPTIONS = [
  { value: 'horizontal', label: 'Horizontal' },
  { value: 'vertical', label: 'Vertical' },
];

const MASK_OPTIONS = [
  { value: 'aperture', label: 'Aperture' },
  { value: 'shadow', label: 'Shadow' },
];

function resolveLineWidth(period: number, params: Record<string, unknown>): number {
  if (params.line_width != null && Number.isFinite(Number(params.line_width))) {
    return clampParam(Math.round(Number(params.line_width)), 1, period);
  }
  // Legacy fractional thickness from the first CRT expansion.
  if (params.thickness != null && Number.isFinite(Number(params.thickness))) {
    const t = clampParam(Number(params.thickness), 0.05, 0.95);
    return clampParam(Math.round(t * period), 1, period);
  }
  return clampParam(Math.max(1, Math.floor(period / 2)), 1, period);
}

function CrtSettings({ params, onUpdate }: CrtSettingsProps) {
  const period = clampParam(Number(params.period) || 2, 2, 8);
  const scale = clampParam(Math.round(Number(params.scale) || 1), 1, 16);
  const lineWidth = resolveLineWidth(period, params);
  const strength = clampParam(Number(params.strength) || 0.5, 0, 1);
  const softScanlines = clampParam(Number(params.soft_scanlines) || 0, 0, 1);
  const orientation =
    params.orientation === 'vertical' ? 'vertical' : 'horizontal';
  const maskType = params.mask_type === 'shadow' ? 'shadow' : 'aperture';
  const maskStrength = clampParam(Number(params.mask_strength) || 0, 0, 1);
  const brightnessCompensate = clampParam(
    Number(params.brightness_compensate) || 0,
    0,
    1,
  );
  const curvature = clampParam(Number(params.curvature) || 0, 0, 1);
  const vignette = clampParam(Number(params.vignette) || 0, 0, 1);
  const phosphorBleed = clampParam(Number(params.phosphor_bleed) || 0, 0, 1);
  const sharpness = clampParam(Number(params.sharpness) || 0, 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    const nextPeriod = clampParam(
      Math.round(Number(overrides.period ?? period)),
      2,
      8,
    );
    const nextLineWidth = clampParam(
      Math.round(Number(overrides.line_width ?? lineWidth)),
      1,
      nextPeriod,
    );
    onUpdate({
      period: nextPeriod,
      scale: clampParam(Math.round(Number(overrides.scale ?? scale)), 1, 16),
      line_width: nextLineWidth,
      strength: overrides.strength ?? strength,
      soft_scanlines: overrides.soft_scanlines ?? softScanlines,
      orientation: overrides.orientation ?? orientation,
      mask_type: overrides.mask_type ?? maskType,
      mask_strength: overrides.mask_strength ?? maskStrength,
      brightness_compensate:
        overrides.brightness_compensate ?? brightnessCompensate,
      curvature: overrides.curvature ?? curvature,
      vignette: overrides.vignette ?? vignette,
      phosphor_bleed: overrides.phosphor_bleed ?? phosphorBleed,
      sharpness: overrides.sharpness ?? sharpness,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <Slider
        label="Scale"
        value={scale}
        min={1}
        max={16}
        step={1}
        decimals={0}
        onChange={(v) => emit({ scale: Math.round(v) })}
      />
      <Slider
        label="Period"
        value={period}
        min={2}
        max={8}
        step={1}
        decimals={0}
        onChange={(v) => emit({ period: Math.round(v) })}
      />
      <Slider
        label="Line Width"
        value={lineWidth}
        min={1}
        max={period}
        step={1}
        decimals={0}
        onChange={(v) => emit({ line_width: Math.round(v) })}
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
        label="Soft Scanlines"
        value={softScanlines}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ soft_scanlines: clampParam(v, 0, 1) })}
      />
      <DropdownMenu
        label="Orientation"
        value={orientation}
        options={ORIENT_OPTIONS}
        onSelect={(v) => emit({ orientation: v })}
        ariaLabel="CRT orientation"
      />
      <DropdownMenu
        label="Mask Type"
        value={maskType}
        options={MASK_OPTIONS}
        onSelect={(v) => emit({ mask_type: v })}
        ariaLabel="CRT mask type"
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
      <Slider
        label="Brightness Comp"
        value={brightnessCompensate}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ brightness_compensate: clampParam(v, 0, 1) })}
      />
      <Slider
        label="Curvature"
        value={curvature}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ curvature: clampParam(v, 0, 1) })}
      />
      <Slider
        label="Vignette"
        value={vignette}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ vignette: clampParam(v, 0, 1) })}
      />
      <Slider
        label="Phosphor Bleed"
        value={phosphorBleed}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ phosphor_bleed: clampParam(v, 0, 1) })}
      />
      <Slider
        label="Sharpness"
        value={sharpness}
        min={0}
        max={1}
        step={0.05}
        decimals={2}
        onChange={(v) => emit({ sharpness: clampParam(v, 0, 1) })}
      />
    </div>
  );
}

export default CrtSettings;
