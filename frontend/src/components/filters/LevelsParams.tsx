import Slider from '../common/Slider';
import styles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

interface LevelsParamsProps {
  inputBlack: number;
  inputWhite: number;
  gamma: number;
  outputBlack: number;
  outputWhite: number;
  onChange: (params: Record<string, unknown>) => void;
}

function LevelsParams({ inputBlack, inputWhite, gamma, outputBlack, outputWhite, onChange }: LevelsParamsProps) {
  const emit = (key: string, value: number) => {
    onChange({
      input_black: inputBlack,
      input_white: inputWhite,
      gamma,
      output_black: outputBlack,
      output_white: outputWhite,
      [key]: value,
    });
  };

  return (
    <div className={cn("filter-params")}>
      <Slider label="Input Black" value={inputBlack} min={0} max={1} step={0.01} onChange={(v) => emit('input_black', v)} />
      <Slider label="Input White" value={inputWhite} min={0} max={1} step={0.01} onChange={(v) => emit('input_white', v)} />
      <Slider label="Gamma" value={gamma} min={0.1} max={10} step={0.1} onChange={(v) => emit('gamma', v)} />
      <Slider label="Output Black" value={outputBlack} min={0} max={1} step={0.01} onChange={(v) => emit('output_black', v)} />
      <Slider label="Output White" value={outputWhite} min={0} max={1} step={0.01} onChange={(v) => emit('output_white', v)} />
    </div>
  );
}

export default LevelsParams;
