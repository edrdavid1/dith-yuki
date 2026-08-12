import { useState } from 'react';
import PaletteSelector from '../PaletteSelector';
import DropdownMenu from '../common/DropdownMenu';
import styles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

interface PaletteQuantizeParamsProps {
  paletteId: number;
  diffusion: string | null;
  onChange: (params: Record<string, unknown>) => void;
}

function PaletteQuantizeParams({ paletteId, diffusion, onChange }: PaletteQuantizeParamsProps) {
  const [selectedPaletteId, setSelectedPaletteId] = useState<number>(paletteId);
  const [selectedDiffusion, setSelectedDiffusion] = useState<string | null>(diffusion);

  const handlePaletteChange = (newId: number) => {
    setSelectedPaletteId(newId);
    onChange({ palette_id: newId, diffusion: selectedDiffusion });
  };

  const handleDiffusionChange = (newDiffusion: string | null) => {
    setSelectedDiffusion(newDiffusion);
    onChange({ palette_id: selectedPaletteId, diffusion: newDiffusion });
  };

  return (
    <div className={cn("filter-params")}>
      <PaletteSelector
        selectedPaletteId={selectedPaletteId}
        allowNone={false}
        onChange={(newId) => {
          if (newId !== null) {
            handlePaletteChange(newId);
          }
        }}
      />

      <DropdownMenu
        label="Diffusion"
        value={selectedDiffusion ?? 'none'}
        options={[
          { value: 'none', label: 'None (Nearest Only)' },
          { value: 'FloydSteinberg', label: 'Floyd-Steinberg' },
          { value: 'Atkinson', label: 'Atkinson' },
          { value: 'JarvisJudiceNinke', label: 'Jarvis-Judice-Ninke' },
          { value: 'Stucki', label: 'Stucki' },
        ]}
        onSelect={(v) => {
          handleDiffusionChange(v === 'none' ? null : v);
        }}
      />

      {selectedPaletteId === 0 && (
        <p style={{ color: '#666', fontSize: '10px', margin: '4px 0 0' }}>
          Add a palette first using the Palette panel below.
        </p>
      )}
    </div>
  );
}

export default PaletteQuantizeParams;
