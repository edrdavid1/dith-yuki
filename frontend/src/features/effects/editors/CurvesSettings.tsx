import { useEffect, useState } from 'react';
import DropdownMenu from '../../../components/common/DropdownMenu';
import NumberInput from '../../../components/common/NumberInput';
import CurveGraph from './CurveGraph';
import {
  fromByte,
  IDENTITY_CURVE,
  movePoint,
  toByte,
  type CurvePoint,
  xBounds,
} from './curveMath';
import { unwrapFilterParams } from '../../../shared/unwrapFilterParams';
import styles from './CurvesSettings.module.css';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import buttonStyles from '../../../shared/ui/FilterButtons.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind({ ...styles, ...panelStyles, ...paramStyles, ...sliderStyles, ...buttonStyles });

interface CurvesSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function asCurve(value: unknown): CurvePoint[] {
  if (!Array.isArray(value) || value.length < 2) return IDENTITY_CURVE.map((p) => [p[0], p[1]]);
  const points: CurvePoint[] = [];
  for (const entry of value) {
    if (Array.isArray(entry) && entry.length >= 2) {
      const x = Number(entry[0]);
      const y = Number(entry[1]);
      if (Number.isFinite(x) && Number.isFinite(y)) points.push([x, y]);
      continue;
    }
    if (entry && typeof entry === 'object') {
      const rec = entry as Record<string, unknown>;
      const x = Number(rec.x ?? rec[0]);
      const y = Number(rec.y ?? rec[1]);
      if (Number.isFinite(x) && Number.isFinite(y)) points.push([x, y]);
    }
  }
  if (points.length < 2) return IDENTITY_CURVE.map((p) => [p[0], p[1]]);
  return points;
}

function curveFromParams(params: Record<string, unknown>): CurvePoint[] {
  const flat = unwrapFilterParams(params);
  return asCurve(flat.curve);
}

function channelFromParams(params: Record<string, unknown>): string {
  const flat = unwrapFilterParams(params);
  return typeof flat.channel === 'string' ? flat.channel : 'All';
}

function CurvesSettings({ params, onUpdate }: CurvesSettingsProps) {
  const channel = channelFromParams(params);
  const curveKey = JSON.stringify(curveFromParams(params));
  const [curve, setCurve] = useState<CurvePoint[]>(() => curveFromParams(params));
  const [selectedIndex, setSelectedIndex] = useState<number | null>(0);
  const selected = selectedIndex !== null ? curve[selectedIndex] ?? null : null;

  useEffect(() => {
    setCurve(asCurve(JSON.parse(curveKey)));
  }, [curveKey]);

  const emit = (overrides: Record<string, unknown>) => {
    const nextCurve = (overrides.curve as CurvePoint[] | undefined) ?? curve;
    if (overrides.curve) setCurve(nextCurve);
    onUpdate({
      curve: nextCurve,
      channel: overrides.channel ?? channel,
    });
  };

  const handleCurveChange = (next: CurvePoint[]) => {
    emit({ curve: next });
  };

  const handleInput = (byte: number) => {
    if (selectedIndex === null) return;
    handleCurveChange(movePoint(curve, selectedIndex, fromByte(byte), curve[selectedIndex][1]));
  };

  const handleOutput = (byte: number) => {
    if (selectedIndex === null) return;
    handleCurveChange(movePoint(curve, selectedIndex, curve[selectedIndex][0], fromByte(byte)));
  };

  const handleReset = () => {
    setSelectedIndex(0);
    emit({ curve: IDENTITY_CURVE.map((p) => [p[0], p[1]]) });
  };

  const inputBounds = selectedIndex !== null ? xBounds(curve, selectedIndex) : { min: 0, max: 1 };

  return (
    <div className={cn('effect-settings-content')}>
      <DropdownMenu
        label="Channel"
        value={channel}
        options={[
          { value: 'All', label: 'All' },
          { value: 'Red', label: 'Red' },
          { value: 'Green', label: 'Green' },
          { value: 'Blue', label: 'Blue' },
          { value: 'Luminance', label: 'Luminance' },
        ]}
        onSelect={(v) => emit({ channel: v })}
      />

      <div className={cn('curve-editor')}>
        <CurveGraph
          curve={curve}
          selectedIndex={selectedIndex}
          channel={channel}
          onChange={handleCurveChange}
          onSelect={setSelectedIndex}
        />

        <div className={cn('curve-io-row')}>
          <NumberInput
            label="Input"
            value={selected ? toByte(selected[0]) : 0}
            min={toByte(inputBounds.min)}
            max={toByte(inputBounds.max)}
            step={1}
            decimals={0}
            disabled={!selected}
            onChange={handleInput}
          />
          <NumberInput
            label="Output"
            value={selected ? toByte(selected[1]) : 0}
            min={0}
            max={255}
            step={1}
            decimals={0}
            disabled={!selected}
            onChange={handleOutput}
          />
          <button type="button" className={cn('filter-add-btn', 'curve-reset-btn')} onClick={handleReset}>
            Reset
          </button>
        </div>
      </div>
    </div>
  );
}

export default CurvesSettings;
