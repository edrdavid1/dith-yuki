import Slider from '../../components/common/Slider';
import { useAlgorithmSchema } from './hooks/useAlgorithmSchema';
import type { ParamField } from '../../shared/ipc/registry';
import panelStyles from './EffectSettingsPanel.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import { bind } from '../../shared/ui/cn';
import { clampParam } from '../../types/effects';

const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

export interface AlgorithmSettingsPanelProps {
  algorithmId: string;
  values: Record<string, unknown>;
  onChange: (patch: Record<string, unknown>) => void;
}

function sliderDecimals(step: number | null): number {
  if (step == null) return 2;
  if (Number.isInteger(step)) return 0;
  const s = step.toString();
  const dot = s.indexOf('.');
  return dot < 0 ? 2 : Math.min(4, s.length - dot - 1);
}

function readNumber(
  values: Record<string, unknown>,
  field: Extract<ParamField, { type: 'slider' }>
): number {
  const raw = values[field.key];
  const n = typeof raw === 'number' ? raw : Number(raw);
  const fallback = Number.isFinite(n) ? n : field.default;
  return clampParam(fallback, field.min, field.max);
}

function readBool(
  values: Record<string, unknown>,
  field: Extract<ParamField, { type: 'checkbox' }>
): boolean {
  const raw = values[field.key];
  if (typeof raw === 'boolean') return raw;
  return field.default;
}

function readChoice(
  values: Record<string, unknown>,
  field: Extract<ParamField, { type: 'dropdown' }>
): string {
  const raw = values[field.key];
  if (typeof raw === 'string' && raw.length > 0) return raw;
  return field.default;
}

function FieldControl({
  field,
  values,
  onChange,
}: {
  field: ParamField;
  values: Record<string, unknown>;
  onChange: (patch: Record<string, unknown>) => void;
}) {
  switch (field.type) {
    case 'slider': {
      const step = field.step == null || field.step <= 0 ? 0.01 : field.step;
      const value = readNumber(values, field);
      return (
        <Slider
          label={field.label}
          value={value}
          min={field.min}
          max={field.max}
          step={step}
          decimals={sliderDecimals(field.step)}
          onChange={(v) => {
            const snapped =
              field.step != null && Number.isInteger(field.step)
                ? Math.round(clampParam(v, field.min, field.max))
                : clampParam(v, field.min, field.max);
            onChange({ [field.key]: snapped });
          }}
        />
      );
    }
    case 'checkbox': {
      const checked = readBool(values, field);
      return (
        <label className={cn('param-checkbox-row')}>
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => onChange({ [field.key]: e.target.checked })}
          />
          {field.label}
        </label>
      );
    }
    case 'dropdown': {
      const value = readChoice(values, field);
      return (
        <div className={cn('param-group')}>
          <label className={cn('slider-label')}>{field.label}</label>
          <select
            className={cn('param-select')}
            value={value}
            onChange={(e) => onChange({ [field.key]: e.target.value })}
          >
            {field.options.map(([optValue, optLabel]) => (
              <option key={optValue} value={optValue}>
                {optLabel}
              </option>
            ))}
          </select>
        </div>
      );
    }
    default:
      return null;
  }
}

/**
 * Schema-driven settings editor. Control types come from `param_schema`
 * (Req 5.2, 5.3) — no per-algorithm JSX.
 */
export default function AlgorithmSettingsPanel({
  algorithmId,
  values,
  onChange,
}: AlgorithmSettingsPanelProps) {
  const { schema, error } = useAlgorithmSchema(algorithmId);

  if (error) {
    return <div className={cn('effect-settings-content')}>unknown effect</div>;
  }

  if (schema == null) {
    return (
      <div className={cn('effect-settings-content')} role="status">
        Loading…
      </div>
    );
  }

  return (
    <div className={cn('effect-settings-content')}>
      {schema.map((field) => (
        <FieldControl key={field.key} field={field} values={values} onChange={onChange} />
      ))}
    </div>
  );
}
