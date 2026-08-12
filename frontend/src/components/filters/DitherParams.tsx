import { useState } from 'react';
import Slider from '../common/Slider';
import DropdownMenu from '../common/DropdownMenu';
import type { DitherMode, DiffusionKernel } from '../../types';
import { open } from '@tauri-apps/plugin-dialog';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import inputStyles from '../../shared/ui/ParamInput.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import buttonStyles from '../../shared/ui/FilterButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...paramStyles, ...inputStyles, ...sliderStyles, ...buttonStyles });

interface DitherParamsProps {
  // New mode-based params
  mode?: DitherMode;
  kernel?: DiffusionKernel;
  matrixSize?: number;
  path?: string;
  colorDepth: number;
  // Legacy
  algorithm?: string;
  onChange: (params: Record<string, unknown>) => void;
}

function DitherParams({ mode: modeProp, kernel: kernelProp, matrixSize: matrixSizeProp, path: pathProp, colorDepth, algorithm, onChange }: DitherParamsProps) {
  // Derive initial mode from legacy algorithm field if mode not set
  const initialMode: DitherMode = modeProp ?? (
    algorithm === 'Ordered' ? 'Bayer' :
    algorithm === 'Threshold' ? 'ThresholdMap' :
    'ErrorDiffusion'
  );

  const [mode, setMode] = useState<DitherMode>(initialMode);
  const [kernel, setKernel] = useState<DiffusionKernel>(kernelProp ?? 'FloydSteinberg');
  const [matrixSize, setMatrixSize] = useState<number>(matrixSizeProp ?? 4);
  const [thresholdPath, setThresholdPath] = useState<string>(pathProp ?? '');

  const emitChange = (newMode: DitherMode, newKernel: DiffusionKernel, newMatrix: number, newPath: string, newDepth: number) => {
    const params: Record<string, unknown> = { mode: newMode, color_depth: newDepth };
    if (newMode === 'ErrorDiffusion') {
      params.kernel = newKernel;
    } else if (newMode === 'Bayer') {
      params.matrix_size = newMatrix;
    } else if (newMode === 'ThresholdMap') {
      params.path = newPath;
    }
    onChange(params);
  };

  const handleModeChange = (newMode: DitherMode) => {
    setMode(newMode);
    emitChange(newMode, kernel, matrixSize, thresholdPath, colorDepth);
  };

  const handleKernelChange = (newKernel: DiffusionKernel) => {
    setKernel(newKernel);
    emitChange(mode, newKernel, matrixSize, thresholdPath, colorDepth);
  };

  const handleMatrixChange = (newSize: number) => {
    setMatrixSize(newSize);
    emitChange(mode, kernel, newSize, thresholdPath, colorDepth);
  };

  const handlePathBrowse = async () => {
    const selected = await open({
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'bmp'] }],
      multiple: false,
    });
    if (selected && typeof selected === 'string') {
      setThresholdPath(selected);
      emitChange(mode, kernel, matrixSize, selected, colorDepth);
    }
  };

  return (
    <div className={cn("filter-params")}>
      <DropdownMenu
        label="Mode"
        value={mode}
        options={[
          { value: 'ErrorDiffusion', label: 'Error Diffusion' },
          { value: 'Bayer', label: 'Bayer (Ordered)' },
          { value: 'ThresholdMap', label: 'Threshold Map' },
        ]}
        onSelect={(v) => handleModeChange(v as DitherMode)}
      />

      {mode === 'ErrorDiffusion' && (
        <DropdownMenu
          label="Kernel"
          value={kernel}
          options={[
            { value: 'FloydSteinberg', label: 'Floyd-Steinberg' },
            { value: 'Atkinson', label: 'Atkinson' },
            { value: 'JarvisJudiceNinke', label: 'Jarvis-Judice-Ninke' },
            { value: 'Stucki', label: 'Stucki' },
          ]}
          onSelect={(v) => handleKernelChange(v as DiffusionKernel)}
        />
      )}

      {mode === 'Bayer' && (
        <DropdownMenu
          label="Matrix Size"
          value={String(matrixSize)}
          options={[
            { value: '2', label: '2×2' },
            { value: '4', label: '4×4' },
            { value: '8', label: '8×8' },
          ]}
          onSelect={(v) => handleMatrixChange(Number(v))}
        />
      )}

      {mode === 'ThresholdMap' && (
        <div className={cn("param-group")}>
          <label className={cn("slider-label")}>Map File</label>
          <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
            <input
              type="text"
              className={cn("param-input")}
              value={thresholdPath}
              readOnly
              placeholder="Select a threshold map..."
              style={{ flex: 1, fontSize: '10px', padding: '2px 4px' }}
            />
            <button className={cn("filter-add-btn")} onClick={handlePathBrowse} style={{ whiteSpace: 'nowrap' }}>
              Browse
            </button>
          </div>
        </div>
      )}

      <Slider
        label="Color Depth (bits)"
        value={colorDepth}
        min={1}
        max={8}
        step={1}
        decimals={0}
        onChange={(val) => {
          const clamped = Math.round(Math.max(1, Math.min(8, val)));
          emitChange(mode, kernel, matrixSize, thresholdPath, clamped);
        }}
      />
    </div>
  );
}

export default DitherParams;
