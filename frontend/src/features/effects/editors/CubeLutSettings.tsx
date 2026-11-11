import { openDialog } from '../../../shared/ipc';
import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import inputStyles from '../../../shared/ui/ParamInput.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import buttonStyles from '../../../shared/ui/FilterButtons.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({
  ...panelStyles,
  ...paramStyles,
  ...inputStyles,
  ...sliderStyles,
  ...buttonStyles,
});

interface CubeLutSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) ? v : fallback;
}

function basename(path: string): string {
  const normalized = path.replace(/\\/g, '/');
  const parts = normalized.split('/');
  return parts[parts.length - 1] || path;
}

async function pickCubeFile(): Promise<string | null> {
  const selected = await openDialog({
    filters: [{ name: 'CUBE LUT', extensions: ['cube'] }],
    multiple: false,
  });
  return typeof selected === 'string' && selected.length > 0 ? selected : null;
}

function CubeLutSettings({ params, onUpdate }: CubeLutSettingsProps) {
  const path = typeof params.path === 'string' ? params.path : '';
  const amount = clampParam(num(params, 'amount', 1), 0, 1);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      path: overrides.path ?? path,
      amount: overrides.amount ?? amount,
    });
  };

  const handleBrowse = async () => {
    const selected = await pickCubeFile();
    if (selected) emit({ path: selected });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <div className={cn('threshold-map-row')}>
        <span className={cn('threshold-map-label')}>LUT File</span>
        <div className={cn('threshold-map-controls')}>
          <input
            className={cn('param-input')}
            type="text"
            readOnly
            value={path ? basename(path) : ''}
            title={path || undefined}
            placeholder="Select a .cube file…"
            aria-label="CUBE LUT path"
          />
          <button
            type="button"
            className={cn('filter-add-btn')}
            onClick={() => {
              void handleBrowse();
            }}
          >
            Browse
          </button>
        </div>
        <p className={cn('effect-palette-hint')}>
          Adobe `.cube` 3D LUT (size ≤65). Embedded into the project on save.
        </p>
      </div>
      <Slider
        label="Amount"
        value={amount}
        min={0}
        max={1}
        step={0.01}
        decimals={2}
        onChange={(v) => emit({ amount: clampParam(v, 0, 1) })}
      />
    </div>
  );
}

export default CubeLutSettings;
