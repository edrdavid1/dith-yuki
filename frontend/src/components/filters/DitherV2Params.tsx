import { useState } from 'react';
import Slider from '../common/Slider';
import DropdownMenu from '../common/DropdownMenu';
import PaletteSelector from '../PaletteSelector';
import type { DitherModeV2, DitherColorMode } from '../../types';
import { open } from '@tauri-apps/plugin-dialog';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import inputStyles from '../../shared/ui/ParamInput.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import buttonStyles from '../../shared/ui/FilterButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...paramStyles, ...inputStyles, ...sliderStyles, ...buttonStyles });

/** Simple string modes for the dropdown. */
type SimpleDitherMode =
  | 'bayer_2x2'
  | 'bayer_4x4'
  | 'bayer_8x8'
  | 'custom_png'
  | 'floyd_steinberg'
  | 'atkinson'
  | 'jarvis_judice_ninke'
  | 'stucki'
  | 'burkes'
  | 'sierra';

interface DitherV2ParamsProps {
  mode: DitherModeV2;
  levels: number;
  thresholdScale: number;
  pixelSize: number;
  colorMode: DitherColorMode;
  paletteId: number | null;
  onChange: (params: Record<string, unknown>) => void;
}

/** Extract a simple string key from the mode for dropdown display. */
function modeToSimple(mode: DitherModeV2): SimpleDitherMode {
  if (typeof mode === 'string') {
    const allowed: SimpleDitherMode[] = [
      'bayer_2x2',
      'bayer_4x4',
      'bayer_8x8',
      'custom_png',
      'floyd_steinberg',
      'atkinson',
      'jarvis_judice_ninke',
      'stucki',
      'burkes',
      'sierra',
    ];
    if (allowed.includes(mode as SimpleDitherMode)) return mode as SimpleDitherMode;
  }
  if (typeof mode === 'object' && mode !== null && 'custom_png' in mode) return 'custom_png';
  return 'bayer_4x4';
}

/** Extract the custom_png path from the mode, if applicable. */
function modeToCustomPath(mode: DitherModeV2): string {
  if (typeof mode === 'object' && 'custom_png' in mode) return mode.custom_png.path;
  return '';
}

/** Convert a simple mode key + custom path back to DitherModeV2. */
function simpleToMode(simple: SimpleDitherMode, customPath: string): DitherModeV2 {
  if (simple === 'custom_png') return { custom_png: { path: customPath } };
  return simple;
}

function DitherV2Params({ mode, levels, thresholdScale, pixelSize, colorMode, paletteId, onChange }: DitherV2ParamsProps) {
  const [simpleMode, setSimpleMode] = useState<SimpleDitherMode>(modeToSimple(mode));
  const [customPath, setCustomPath] = useState<string>(modeToCustomPath(mode));

  const emit = (overrides: Partial<{
    mode: DitherModeV2;
    levels: number;
    threshold_scale: number;
    pixel_size: number;
    color_mode: DitherColorMode;
    palette_id: number | null;
  }>) => {
    onChange({
      mode: overrides.mode ?? simpleToMode(simpleMode, customPath),
      levels: overrides.levels ?? levels,
      threshold_scale: overrides.threshold_scale ?? thresholdScale,
      pixel_size: overrides.pixel_size ?? pixelSize,
      color_mode: overrides.color_mode ?? colorMode,
      palette_id: overrides.palette_id !== undefined ? overrides.palette_id : paletteId,
    });
  };

  const handleModeChange = (newSimple: SimpleDitherMode) => {
    setSimpleMode(newSimple);
    emit({ mode: simpleToMode(newSimple, customPath) });
  };

  const handlePathBrowse = async () => {
    const selected = await open({
      filters: [{ name: 'PNG Images', extensions: ['png'] }],
      multiple: false,
    });
    if (selected && typeof selected === 'string') {
      setCustomPath(selected);
      emit({ mode: { custom_png: { path: selected } } });
    }
  };

  const handleColorModeToggle = () => {
    const newMode: DitherColorMode = colorMode === 'rgb' ? 'grayscale' : 'rgb';
    emit({ color_mode: newMode });
  };

  return (
    <div className={cn("filter-params")}>
      {/* Mode Selector */}
      <DropdownMenu
        label="Mode"
        value={simpleMode}
        options={[
          { value: 'bayer_2x2', label: 'Bayer 2×2' },
          { value: 'bayer_4x4', label: 'Bayer 4×4' },
          { value: 'bayer_8x8', label: 'Bayer 8×8' },
          { value: 'custom_png', label: 'Custom Threshold Map' },
          { value: 'floyd_steinberg', label: 'Floyd-Steinberg' },
          { value: 'atkinson', label: 'Atkinson' },
          { value: 'jarvis_judice_ninke', label: 'Jarvis-Judice-Ninke' },
          { value: 'stucki', label: 'Stucki' },
          { value: 'burkes', label: 'Burkes' },
          { value: 'sierra', label: 'Sierra' },
        ]}
        onSelect={(v) => handleModeChange(v as SimpleDitherMode)}
      />

      {/* Custom PNG path selector */}
      {simpleMode === 'custom_png' && (
        <div className={cn("param-group")}>
          <label className={cn("slider-label")}>Threshold Map</label>
          <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
            <input
              type="text"
              className={cn("param-input")}
              value={customPath}
              readOnly
              placeholder="Select a grayscale PNG..."
              style={{ flex: 1, fontSize: '10px', padding: '2px 4px' }}
            />
            <button className={cn("filter-add-btn")} onClick={handlePathBrowse} style={{ whiteSpace: 'nowrap' }}>
              Browse
            </button>
          </div>
        </div>
      )}

      {/* Levels slider */}
      <Slider
        label="Levels"
        value={levels}
        min={2}
        max={256}
        step={1}
        decimals={0}
        onChange={(v) => emit({ levels: Math.round(v) })}
      />

      {/* Threshold Scale slider */}
      <Slider
        label="Threshold Scale"
        value={thresholdScale}
        min={0.1}
        max={4.0}
        step={0.1}
        decimals={1}
        onChange={(v) => emit({ threshold_scale: v })}
      />

      {/* Pixel Size slider */}
      <Slider
        label="Pixel Size"
        value={pixelSize}
        min={1}
        max={32}
        step={1}
        decimals={0}
        onChange={(v) => emit({ pixel_size: Math.round(v) })}
      />

      {/* Color Mode toggle */}
      <div className={cn("param-group")}>
        <label className={cn("slider-label")}>Color Mode</label>
        <button
          className={cn("param-select")}
          onClick={handleColorModeToggle}
          style={{ cursor: 'pointer', textAlign: 'left' }}
        >
          {colorMode === 'rgb' ? 'RGB' : 'Grayscale'}
        </button>
      </div>

      {/* Palette Selector */}
      <PaletteSelector
        selectedPaletteId={paletteId}
        allowNone={true}
        onChange={(newPaletteId) => emit({ palette_id: newPaletteId })}
      />
    </div>
  );
}

export default DitherV2Params;
