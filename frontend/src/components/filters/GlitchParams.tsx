import Slider from '../common/Slider';
import DropdownMenu from '../common/DropdownMenu';
import type { GlitchType } from '../../types';
import styles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

interface GlitchParamsProps {
  glitchType: GlitchType;
  intensity: number;
  seed: number;
  onChange: (params: Record<string, unknown>) => void;
}

function GlitchParams({ glitchType, intensity, seed, onChange }: GlitchParamsProps) {
  return (
    <div className={cn("filter-params")}>
      <DropdownMenu
        label="Effect Type"
        value={glitchType}
        options={[
          { value: 'RGBShift', label: 'RGB Shift' },
          { value: 'BlockDisplace', label: 'Block Displacement' },
        ]}
        onSelect={(v) => onChange({ glitch_type: v, intensity, seed })}
      />
      <Slider
        label="Intensity"
        value={intensity}
        min={0}
        max={1}
        step={0.01}
        onChange={(val) => onChange({ glitch_type: glitchType, intensity: val, seed })}
      />
    </div>
  );
}

export default GlitchParams;
