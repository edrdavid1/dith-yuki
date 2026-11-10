import Slider from '../common/Slider';
import NumberInput from '../common/NumberInput';
import DropdownMenu from '../common/DropdownMenu';
import styles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

interface GlitchSubOp {
  enabled: boolean;
  amount: number;
}

interface GlitchParamsProps {
  enabled: boolean;
  seed: number;
  blockSize: number;
  groupSize: number;
  blockShift: GlitchSubOp;
  blockScramble: GlitchSubOp;
  channelSwap: GlitchSubOp;
  scanline: GlitchSubOp;
  interlace: GlitchSubOp;
  onChange: (params: Record<string, unknown>) => void;
}

function GlitchParams({
  enabled,
  seed,
  blockSize,
  groupSize,
  blockShift,
  blockScramble,
  channelSwap,
  scanline,
  interlace,
  onChange,
}: GlitchParamsProps) {
  const base = () => ({
    enabled,
    seed,
    block_size: blockSize,
    group_size: groupSize,
    block_shift: blockShift,
    block_scramble: blockScramble,
    channel_swap: channelSwap,
    scanline,
    interlace,
  });

  return (
    <div className={cn('filter-params')}>
      <DropdownMenu
        label="Block Size"
        value={String(blockSize)}
        options={[
          { value: '8', label: '8' },
          { value: '16', label: '16' },
          { value: '32', label: '32' },
        ]}
        onSelect={(v) => onChange({ ...base(), block_size: Number(v) })}
      />
      <DropdownMenu
        label="Group Size"
        value={String(groupSize)}
        options={[
          { value: '2', label: '2' },
          { value: '4', label: '4' },
          { value: '8', label: '8' },
        ]}
        onSelect={(v) => onChange({ ...base(), group_size: Number(v) })}
      />
      <Slider
        label="Channel Swap"
        value={channelSwap.amount}
        min={0}
        max={100}
        step={1}
        onChange={(val) =>
          onChange({
            ...base(),
            channel_swap: { enabled: true, amount: val },
          })
        }
      />
      <NumberInput
        label="Seed"
        value={seed}
        min={0}
        max={99999}
        step={1}
        decimals={0}
        onChange={(val) => onChange({ ...base(), seed: Math.round(val) })}
      />
    </div>
  );
}

export default GlitchParams;
