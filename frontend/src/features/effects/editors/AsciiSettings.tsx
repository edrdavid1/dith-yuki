import { clampParam } from '../../../types/effects';
import Slider from '../../../components/common/Slider';
import DropdownMenu from '../../../components/common/DropdownMenu';
import panelStyles from '../EffectSettingsPanel.module.css';
import paramStyles from '../../../shared/ui/ParamControls.module.css';
import sliderStyles from '../../../shared/ui/Slider.module.css';
import { bind } from '../../../shared/ui/cn';
const cn = bind({ ...panelStyles, ...paramStyles, ...sliderStyles });

interface AsciiSettingsProps {
  params: Record<string, unknown>;
  onUpdate: (params: Record<string, unknown>) => void;
}

function AsciiSettings({ params, onUpdate }: AsciiSettingsProps) {
  const font = (params.font as string) || 'departure_mono';
  const sizeMode = (params.size_mode as string) || 'px';
  const fontPx = clampParam(Number(params.font_px) || 11, 4, 64);
  const columns = clampParam(Number(params.columns) || 120, 20, 320);
  const antialias = params.antialias === true;
  const hinting = params.hinting === true;
  const symbolSet = (params.symbol_set as string) || 'bourke_70';
  const matchMode = (params.match_mode as string) || 'shape';
  const contrast = clampParam(Number(params.contrast) || 1, 0.25, 4);
  const colorMode = (params.color_mode as string) || 'mono';
  const colorTarget = (params.color_target as string) || 'truecolor';
  const cellDither = (params.cell_dither as string) || 'none';
  const serpentine = params.serpentine === true;
  const edgeOverlay = params.edge_overlay === true;
  const edgeTau = clampParam(Number(params.edge_tau) || 40, 0, 255);

  const emit = (overrides: Record<string, unknown>) => {
    onUpdate({
      font: overrides.font ?? font,
      size_mode: overrides.size_mode ?? sizeMode,
      font_px: overrides.font_px ?? fontPx,
      columns: overrides.columns ?? columns,
      antialias: overrides.antialias ?? antialias,
      hinting: overrides.hinting ?? hinting,
      symbol_set: overrides.symbol_set ?? symbolSet,
      match_mode: overrides.match_mode ?? matchMode,
      contrast: overrides.contrast ?? contrast,
      color_mode: overrides.color_mode ?? colorMode,
      color_target: overrides.color_target ?? colorTarget,
      cell_dither: overrides.cell_dither ?? cellDither,
      serpentine: overrides.serpentine ?? serpentine,
      edge_overlay: overrides.edge_overlay ?? edgeOverlay,
      edge_tau: overrides.edge_tau ?? edgeTau,
    });
  };

  return (
    <div className={cn('effect-settings-content')}>
      <p className={cn('effect-palette-hint')}>
        ASCII runs as a full-document pass — preview updates after the grid is ready.
      </p>
      <DropdownMenu
        label="Font"
        value={font}
        options={[
          { value: 'departure_mono', label: 'Departure Mono' },
          { value: 'ibm_plex_mono', label: 'IBM Plex Mono' },
        ]}
        onSelect={(v) =>
          emit({
            font: v,
            // Pixel font: crisp defaults; outline: AA + hinting on.
            antialias: v === 'ibm_plex_mono',
            hinting: v === 'ibm_plex_mono',
            font_px: v === 'departure_mono' ? 11 : fontPx,
          })
        }
      />

      <DropdownMenu
        label="Size Mode"
        value={sizeMode}
        options={[
          { value: 'px', label: 'Font size (px)' },
          { value: 'columns', label: 'Columns' },
        ]}
        onSelect={(v) => emit({ size_mode: v })}
      />

      {sizeMode === 'px' ? (
        <Slider
          label="Font Size"
          value={fontPx}
          min={4}
          max={64}
          step={1}
          decimals={0}
          onChange={(v) => emit({ font_px: clampParam(v, 4, 64) })}
        />
      ) : (
        <Slider
          label="Columns"
          value={columns}
          min={20}
          max={320}
          step={1}
          decimals={0}
          onChange={(v) => emit({ columns: clampParam(v, 20, 320) })}
        />
      )}

      <DropdownMenu
        label="Symbol Set"
        value={symbolSet}
        options={[
          { value: 'bourke_10', label: 'Bourke 10' },
          { value: 'bourke_70', label: 'Bourke 70' },
          { value: 'printable_ascii', label: 'Printable ASCII' },
          { value: 'ascii_box_drawing', label: 'ASCII + Box Drawing' },
          { value: 'blocks', label: 'Blocks' },
          { value: 'quadrants', label: 'Quadrants' },
          { value: 'sextants', label: 'Sextants' },
          { value: 'octants', label: 'Octants' },
          { value: 'braille', label: 'Braille' },
          { value: 'cp437', label: 'CP437' },
        ]}
        onSelect={(v) => emit({ symbol_set: v })}
      />

      <DropdownMenu
        label="Match Mode"
        value={matchMode}
        options={[
          { value: 'tone', label: 'Tone' },
          { value: 'shape', label: 'Shape' },
          { value: 'shape_contrast', label: 'Shape + Contrast' },
          { value: 'mask_two_color', label: 'Mask Two-Color' },
        ]}
        onSelect={(v) => emit({ match_mode: v })}
      />

      {matchMode === 'shape_contrast' && (
        <Slider
          label="Contrast"
          value={contrast}
          min={0.25}
          max={4}
          step={0.05}
          decimals={2}
          onChange={(v) => emit({ contrast: clampParam(v, 0.25, 4) })}
        />
      )}

      <DropdownMenu
        label="Color Mode"
        value={colorMode}
        options={[
          { value: 'mono', label: 'Mono' },
          { value: 'fg', label: 'Foreground' },
          { value: 'fg_bg', label: 'Foreground + Background' },
        ]}
        onSelect={(v) => emit({ color_mode: v })}
      />

      <DropdownMenu
        label="Color Target"
        value={colorTarget}
        options={[
          { value: 'truecolor', label: 'TrueColor' },
          { value: 'xterm256', label: 'xterm 256' },
          { value: 'ansi16_vga', label: 'ANSI-16 VGA' },
          { value: 'ansi16_xterm', label: 'ANSI-16 xterm' },
          { value: 'ansi16_win10', label: 'ANSI-16 Windows 10' },
        ]}
        onSelect={(v) => emit({ color_target: v })}
      />

      <DropdownMenu
        label="Cell Dither"
        value={cellDither}
        options={[
          { value: 'none', label: 'None' },
          { value: 'bayer2', label: 'Bayer 2×2' },
          { value: 'bayer4', label: 'Bayer 4×4' },
          { value: 'bayer8', label: 'Bayer 8×8' },
          { value: 'floyd_steinberg', label: 'Floyd–Steinberg' },
        ]}
        onSelect={(v) => emit({ cell_dither: v })}
      />

      {cellDither === 'floyd_steinberg' && (
        <label className={cn('param-checkbox-row')}>
          <input
            type="checkbox"
            checked={serpentine}
            onChange={(e) => emit({ serpentine: e.target.checked })}
            aria-label="Serpentine"
          />
          <span>Serpentine</span>
        </label>
      )}

      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={edgeOverlay}
          onChange={(e) => emit({ edge_overlay: e.target.checked })}
          aria-label="Edge Overlay"
        />
        <span>Edge Overlay</span>
      </label>

      {edgeOverlay && (
        <Slider
          label="Edge Threshold"
          value={edgeTau}
          min={0}
          max={255}
          step={1}
          decimals={0}
          onChange={(v) => emit({ edge_tau: clampParam(v, 0, 255) })}
        />
      )}

      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={antialias}
          onChange={(e) => emit({ antialias: e.target.checked })}
          aria-label="Antialias"
        />
        <span>Antialias</span>
      </label>
      <label className={cn('param-checkbox-row')}>
        <input
          type="checkbox"
          checked={hinting}
          onChange={(e) => emit({ hinting: e.target.checked })}
          aria-label="Hinting"
        />
        <span>Hinting</span>
      </label>
    </div>
  );
}

export default AsciiSettings;
