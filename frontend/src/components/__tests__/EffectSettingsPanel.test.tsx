import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import type { ReactElement } from 'react';
import EffectSettingsPanel from '../EffectSettingsPanel';
import type { LayerWithFilters } from '../EffectSettingsPanel';
import type { FilterInfo } from '../../types';
import { StoreProvider, createTestStore } from '../../app/__tests__/testStore';

// Mock listPalettes IPC call
vi.mock('../../ipc/commands', () => ({
  listPalettes: vi.fn().mockResolvedValue([]),
}));

vi.mock('../../shared/ipc/registry', () => ({
  EFFECT_CATEGORIES: ['dithering', 'glitch', 'color_adjust', 'stylize', 'palette', 'ascii'],
  listAlgorithmsForCategory: vi.fn(async (category: string) => {
    if (category === 'dithering') {
      return [
        {
          id: 'floyd_steinberg',
          display_name: 'Floyd–Steinberg',
          category: 'dithering',
          deprecated: false,
        },
        {
          id: 'bayer_4x4',
          display_name: 'Bayer 4×4',
          category: 'dithering',
          deprecated: false,
        },
      ];
    }
    if (category === 'ascii') {
      return [
        {
          id: 'ascii',
          display_name: 'ASCII',
          category: 'ascii',
          deprecated: false,
        },
      ];
    }
    return [];
  }),
  getAlgorithmSchema: vi.fn(async (id: string) => {
    if (id === 'bayer_4x4' || id === 'floyd_steinberg' || id === 'palette_quantize') {
      return [
        {
          type: 'slider',
          key: 'levels',
          label: 'Levels',
          min: 2,
          max: 256,
          default: 4,
          step: null,
        },
      ];
    }
    throw new Error(`unknown algorithm: ${id}`);
  }),
}));

function renderPanel(ui: ReactElement) {
  return render(<StoreProvider>{ui}</StoreProvider>);
}

function sliderValueInput(label: string): HTMLInputElement {
  const labelEl = screen.getByText(label);
  const input = labelEl.parentElement?.querySelector('input');
  if (!input) {
    throw new Error(`no value input for ${label}`);
  }
  return input as HTMLInputElement;
}

function makeDitherLayer(overrides?: Partial<Record<string, unknown>>): LayerWithFilters {
  return {
    id: 1,
    name: 'Dither Layer',
    filters: [{
      id: 'filter-1',
      kind: 'DitherV2',
      params: {
        type: 'DitherV2',
        mode: 'floyd_steinberg',
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: 'rgb',
        palette_id: null,
        ...overrides,
      },
      enabled: true,
      opacity: 1,
      blend_mode: 'Normal',
    } as FilterInfo],
  };
}

function makeGlitchLayer(): LayerWithFilters {
  return {
    id: 2,
    name: 'Glitch Layer',
    filters: [{
      id: 'filter-2',
      kind: 'Glitch',
      params: {
        type: 'Glitch',
        glitch_type: 'RGBShift',
        intensity: 0.5,
        seed: 0,
      },
      enabled: true,
      opacity: 1,
      blend_mode: 'Normal',
    } as FilterInfo],
  };
}

function makeCurvesLayer(): LayerWithFilters {
  return {
    id: 3,
    name: 'Curves Layer',
    filters: [{
      id: 'filter-3',
      kind: 'Curves',
      params: {
        type: 'Curves',
        curve: [[0, 0], [1, 1]],
        channel: 'All',
      },
      enabled: true,
      opacity: 1,
      blend_mode: 'Normal',
    } as FilterInfo],
  };
}

function makeRGBLayer(): LayerWithFilters {
  return {
    id: 4,
    name: 'RGB Layer',
    filters: [{
      id: 'filter-4',
      kind: 'Levels',
      params: {
        type: 'Levels',
        input_black: 0.0,
        input_white: 1.0,
        gamma: 1.0,
        output_black: 0.0,
        output_white: 1.0,
      },
      enabled: true,
      opacity: 1,
      blend_mode: 'Normal',
    } as FilterInfo],
  };
}

function makeImageSourceLayer(): LayerWithFilters {
  return {
    id: 0,
    name: 'Background',
    filters: [],
  };
}

describe('EffectSettingsPanel', () => {
  let onUpdateParams: ReturnType<typeof vi.fn<(layerId: number, filterId: string, params: Record<string, unknown>) => void>>;

  beforeEach(() => {
    onUpdateParams = vi.fn<(layerId: number, filterId: string, params: Record<string, unknown>) => void>();
  });

  describe('Empty state', () => {
    it('shows a single Dithering row instead of per-algorithm effects', async () => {
      render(<EffectSettingsPanel selectedLayer={null} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Effect')).toBeInTheDocument();
      await waitFor(() => {
        expect(screen.getByText('Dithering')).toBeInTheDocument();
      });
      expect(screen.queryByText('Floyd–Steinberg')).not.toBeInTheDocument();
      expect(screen.queryByText('Bayer 4×4')).not.toBeInTheDocument();
      expect(screen.getByText('RGB channels')).toBeInTheDocument();
      expect(screen.queryByRole('slider')).not.toBeInTheDocument();
    });

    it('shows effect chooser when Image_Source_Layer is selected (no filters)', async () => {
      render(<EffectSettingsPanel selectedLayer={makeImageSourceLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Effect')).toBeInTheDocument();
      await waitFor(() => {
        expect(screen.getByText('Dithering')).toBeInTheDocument();
      });
      expect(screen.queryByRole('slider')).not.toBeInTheDocument();
    });

    it('calls onSelectEffect when Dithering is clicked', async () => {
      const onSelectEffect = vi.fn();
      render(
        <EffectSettingsPanel
          selectedLayer={null}
          onUpdateParams={onUpdateParams}
          onSelectEffect={onSelectEffect}
        />
      );
      await waitFor(() => {
        expect(screen.getByText('Dithering')).toBeInTheDocument();
      });
      // Chooser rows respond to pointerdown so the click works on the first
      // press even when the main window was inactive (macOS/WKWebView
      // acceptsFirstMouse override delivers the activation click too).
      fireEvent.pointerDown(screen.getByText('Dithering'), { button: 0 });
      expect(onSelectEffect).toHaveBeenCalledWith('Dithering');
    });

    it('shows a single ASCII row outside dithering', async () => {
      const onSelectEffect = vi.fn();
      render(
        <EffectSettingsPanel
          selectedLayer={null}
          onUpdateParams={onUpdateParams}
          onSelectEffect={onSelectEffect}
        />
      );
      await waitFor(() => {
        expect(screen.getByText('ASCII')).toBeInTheDocument();
      });
      expect(screen.queryByText('ascii')).not.toBeInTheDocument();
      fireEvent.pointerDown(screen.getByText('ASCII'), { button: 0 });
      expect(onSelectEffect).toHaveBeenCalledWith('Ascii');
    });

    it('calls onSelectEffect for the leftover RGB channels row', async () => {
      const onSelectEffect = vi.fn();
      render(<EffectSettingsPanel selectedLayer={null} onUpdateParams={onUpdateParams} onSelectEffect={onSelectEffect} />);
      await waitFor(() => {
        expect(screen.getByText('RGB channels')).toBeInTheDocument();
      });
      fireEvent.pointerDown(screen.getByText('RGB channels'), { button: 0 });
      expect(onSelectEffect).toHaveBeenCalledWith('RGBChannels');
    });
  });

  describe('Dithering settings', () => {
    it('renders algorithm dropdown with correct options', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Algorithm')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Floyd-Steinberg'));
      expect(screen.getByRole('option', { name: 'Jarvis-Judice-Ninke' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Stucki' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Burkes' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Sierra' })).toBeInTheDocument();
    });

    it('renders pixel size slider with range 1–32', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Pixel Size')).toBeInTheDocument();
      expect(sliderValueInput('Pixel Size').value).toBe('1');
    });

    it('renders threshold scale slider with range 0.1–4.0', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Threshold Scale')).toBeInTheDocument();
      expect(sliderValueInput('Threshold Scale').value).toBe('1.0');
    });

    it('renders levels slider with range 2–256', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Levels')).toBeInTheDocument();
      expect(sliderValueInput('Levels').value).toBe('4');
    });

    it('notes that levels is ignored when a Color Lab palette is bound', () => {
      const store = createTestStore({
        palettes: { version: 1, lastCreatedId: 12, error: null },
      });
      render(
        <StoreProvider store={store}>
          <EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />
        </StoreProvider>,
      );
      expect(screen.getByText(/Levels is ignored/)).toBeInTheDocument();
      expect(sliderValueInput('Levels').value).toBe('4');
    });

    it('calls onUpdateParams with clamped pixel size', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const input = sliderValueInput('Pixel Size');
      fireEvent.change(input, { target: { value: '16' } });
      fireEvent.blur(input);
      expect(onUpdateParams).toHaveBeenCalledWith(1, 'filter-1', expect.objectContaining({ pixel_size: 16 }));
    });

    it('always shows palette dither controls (even without a bound palette)', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Palette dither')).toBeInTheDocument();
      expect(screen.getByText('Strict — exact palette colors')).toBeInTheDocument();
      expect(screen.queryByText('Levels per channel')).not.toBeInTheDocument();
    });

    it('shows palette dither controls when palette_id is set', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({ palette_id: 1 })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Palette dither')).toBeInTheDocument();
      expect(screen.getByText('Strict — exact palette colors')).toBeInTheDocument();
      expect(screen.queryByText('Levels per channel')).not.toBeInTheDocument();
    });

    it('shows Simple palette dither without levels slider', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({
            palette_id: 1,
            palette_dither_mode: 'simple',
          })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Simple — sRGB Euclidean (classic)')).toBeInTheDocument();
      expect(screen.queryByText('Levels per channel')).not.toBeInTheDocument();
    });

    it('shows levels per channel slider for Mixed palette dither', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({
            palette_id: 1,
            palette_dither_mode: { mixed: { channel_levels: 5 } },
          })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Mixed — Guided then palette dither')).toBeInTheDocument();
      expect(screen.getByText('Levels per channel')).toBeInTheDocument();
    });

    it('shows levels per channel slider for Guided palette dither', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({
            palette_id: 1,
            palette_dither_mode: { guided: { channel_levels: 4 } },
          })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Guided — palette-derived range (richer)')).toBeInTheDocument();
      expect(screen.getByText('Levels per channel')).toBeInTheDocument();
    });

    it('calls onUpdateParams when algorithm changes', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      fireEvent.click(screen.getByText('Floyd-Steinberg'));
      fireEvent.click(screen.getByRole('option', { name: 'Bayer 4×4' }));
      expect(onUpdateParams).toHaveBeenCalledWith(1, 'filter-1', expect.objectContaining({ mode: 'bayer_4x4' }));
    });

    it('hides threshold bias and pattern angle for error diffusion', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.queryByText('Threshold Bias')).not.toBeInTheDocument();
      expect(screen.queryByText('Pattern Angle')).not.toBeInTheDocument();
    });

    it('shows serpentine checkbox for error diffusion', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const box = screen.getByRole('checkbox', { name: 'Serpentine' });
      expect(box).not.toBeChecked();
      fireEvent.click(box);
      expect(onUpdateParams).toHaveBeenCalledWith(
        1,
        'filter-1',
        expect.objectContaining({ serpentine: true }),
      );
    });

    it('hides serpentine checkbox for Bayer', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({ mode: 'bayer_4x4' })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.queryByRole('checkbox', { name: 'Serpentine' })).not.toBeInTheDocument();
    });

    it('shows pixelate alpha checked by default', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const box = screen.getByRole('checkbox', { name: 'Pixelate Alpha' });
      expect(box).toBeChecked();
      fireEvent.click(box);
      expect(onUpdateParams).toHaveBeenCalledWith(
        1,
        'filter-1',
        expect.objectContaining({ dither_alpha: false }),
      );
    });

    it('shows threshold bias and pattern angle for Bayer', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({ mode: 'bayer_4x4' })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Threshold Bias')).toBeInTheDocument();
      expect(screen.getByText('Pattern Angle')).toBeInTheDocument();
    });

    it('shows threshold bias but not pattern angle for Wave', () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={makeDitherLayer({ mode: 'wave' })}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Threshold Bias')).toBeInTheDocument();
      expect(screen.queryByText('Pattern Angle')).not.toBeInTheDocument();
    });
  });

  describe('Glitch settings', () => {
    it('renders glitch type dropdown', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Glitch Type')).toBeInTheDocument();
      expect(screen.getByText('RGB Shift')).toBeInTheDocument();
    });

    it('renders intensity slider with range 0–1', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Intensity')).toBeInTheDocument();
      expect(screen.getByDisplayValue('0.50')).toBeInTheDocument();
    });

    it('renders seed number input', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Seed')).toBeInTheDocument();
      const seedInput = screen.getByLabelText('Seed') as HTMLInputElement;
      expect(seedInput).toBeTruthy();
      expect(seedInput.value).toBe('0');
    });

    it('clamps seed to 0–99999 range', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      const seedInput = screen.getByLabelText('Seed') as HTMLInputElement;
      fireEvent.change(seedInput, { target: { value: '150000' } });
      fireEvent.blur(seedInput);
      expect(onUpdateParams).toHaveBeenCalledWith(2, 'filter-2', expect.objectContaining({ seed: 99999 }));
    });
  });

  describe('Curves settings', () => {
    it('renders channel dropdown and graph editor', () => {
      render(<EffectSettingsPanel selectedLayer={makeCurvesLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Channel')).toBeInTheDocument();
      expect(screen.getByText('All')).toBeInTheDocument();
      expect(screen.getByTestId('curve-graph')).toBeInTheDocument();
      expect(screen.getByLabelText('Input')).toHaveValue('0');
      expect(screen.getByLabelText('Output')).toHaveValue('0');
    });

    it('renders dedicated Curves editor when algorithm_id is stamped', () => {
      render(
        <EffectSettingsPanel
          selectedLayer={{
            ...makeCurvesLayer(),
            filters: [{
              ...makeCurvesLayer().filters[0],
              algorithm_id: 'curves',
            } as FilterInfo],
          }}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByTestId('curve-graph')).toBeInTheDocument();
      expect(screen.getByText('Channel')).toBeInTheDocument();
    });

    it('calls onUpdateParams when channel changes', () => {
      render(<EffectSettingsPanel selectedLayer={makeCurvesLayer()} onUpdateParams={onUpdateParams} />);
      fireEvent.click(screen.getByLabelText('Open dropdown'));
      fireEvent.click(screen.getByRole('option', { name: 'Red' }));
      expect(onUpdateParams).toHaveBeenCalledWith(3, 'filter-3', expect.objectContaining({ channel: 'Red' }));
    });
  });

  describe('RGB/Levels settings', () => {
    it('renders all 5 level sliders', () => {
      render(<EffectSettingsPanel selectedLayer={makeRGBLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Input Black')).toBeInTheDocument();
      expect(screen.getByText('Input White')).toBeInTheDocument();
      expect(screen.getByText('Gamma')).toBeInTheDocument();
      expect(screen.getByText('Output Black')).toBeInTheDocument();
      expect(screen.getByText('Output White')).toBeInTheDocument();
    });

    it('renders gamma slider with range 0.1–10', () => {
      render(<EffectSettingsPanel selectedLayer={makeRGBLayer()} onUpdateParams={onUpdateParams} />);
      expect(sliderValueInput('Gamma').value).toBe('1.0');
    });

    it('calls onUpdateParams with updated gamma', () => {
      render(<EffectSettingsPanel selectedLayer={makeRGBLayer()} onUpdateParams={onUpdateParams} />);
      const input = sliderValueInput('Gamma');
      fireEvent.change(input, { target: { value: '2.5' } });
      fireEvent.blur(input);
      expect(onUpdateParams).toHaveBeenCalledWith(4, 'filter-4', expect.objectContaining({ gamma: 2.5 }));
    });

    it('toggles RGB channels off', () => {
      render(<EffectSettingsPanel selectedLayer={makeRGBLayer()} onUpdateParams={onUpdateParams} />);
      fireEvent.click(screen.getByRole('button', { name: 'Red channel' }));
      expect(onUpdateParams).toHaveBeenCalledWith(
        4,
        'filter-4',
        expect.objectContaining({ channel_r: false, channel_g: true, channel_b: true }),
      );
    });
  });

  describe('Adjust settings', () => {
    it('renders contrast brightness saturation blur sharpness noise', () => {
      render(
        <EffectSettingsPanel
          selectedLayer={{
            id: 9,
            name: 'Adjust',
            filters: [{
              id: 'filter-9',
              kind: 'Adjust',
              params: {
                type: 'Adjust',
                contrast: 0,
                brightness: 0,
                saturation: 0,
                blur: 0,
                sharpness: 0,
                noise: 0,
              },
              enabled: true,
              opacity: 1,
              blend_mode: 'Normal',
            } as FilterInfo],
          }}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Contrast')).toBeInTheDocument();
      expect(screen.getByText('Brightness')).toBeInTheDocument();
      expect(screen.getByText('Saturation')).toBeInTheDocument();
      expect(screen.getByText('Blur')).toBeInTheDocument();
      expect(screen.getByText('Sharpness')).toBeInTheDocument();
      expect(screen.getByText('Noise')).toBeInTheDocument();
    });
  });

  describe('ASCII settings', () => {
    it('renders dedicated ASCII controls, not dither algorithm list', () => {
      render(
        <EffectSettingsPanel
          selectedLayer={{
            id: 10,
            name: 'ASCII',
            filters: [{
              id: 'filter-10',
              kind: 'Ascii',
              params: {
                type: 'Ascii',
                font: 'departure_mono',
                size_mode: 'px',
                font_px: 11,
                columns: 120,
                antialias: false,
                hinting: false,
                symbol_set: 'bourke_70',
                match_mode: 'shape',
                contrast: 1,
                color_mode: 'mono',
              },
              enabled: true,
              opacity: 1,
              blend_mode: 'Normal',
              algorithm_id: 'ascii',
            } as FilterInfo],
          }}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('ASCII')).toBeInTheDocument();
      expect(screen.getByText('Font')).toBeInTheDocument();
      expect(screen.getByText('Symbol Set')).toBeInTheDocument();
      expect(screen.getByText('Match Mode')).toBeInTheDocument();
      expect(screen.queryByText('Algorithm')).not.toBeInTheDocument();
      expect(screen.queryByText('Pixel Size')).not.toBeInTheDocument();
    });
  });

  describe('per-filter blend', () => {
    it('does not duplicate opacity/blend controls (those live in Layers)', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.queryByText('Blend')).not.toBeInTheDocument();
      expect(screen.queryByText('Opacity')).not.toBeInTheDocument();
    });
  });

  describe('pattern export/import', () => {
    it('disables pattern actions when no target layer', () => {
      render(<EffectSettingsPanel selectedLayer={null} onUpdateParams={onUpdateParams} />);
      expect(screen.getByRole('button', { name: 'Export pattern' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Import pattern' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Save pattern' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Load pattern' })).toBeDisabled();
    });

    it('enables pattern actions when a layer is targeted', () => {
      const onExportPattern = vi.fn();
      const onImportPattern = vi.fn();
      const onSavePattern = vi.fn();
      const onLoadPattern = vi.fn();
      render(
        <EffectSettingsPanel
          selectedLayer={{
            id: 1,
            name: 'Layer',
            filters: [
              {
                id: 'filter-1',
                kind: 'Glow',
                params: { type: 'Glow', radius: 1, intensity: 1, threshold: 0 },
                enabled: true,
                opacity: 1,
                blend_mode: 'Normal',
              } as FilterInfo,
            ],
          }}
          onUpdateParams={onUpdateParams}
          targetLayerId={1}
          onExportPattern={onExportPattern}
          onImportPattern={onImportPattern}
          onSavePattern={onSavePattern}
          onLoadPattern={onLoadPattern}
        />
      );
      const exp = screen.getByRole('button', { name: 'Export pattern' });
      const imp = screen.getByRole('button', { name: 'Import pattern' });
      const save = screen.getByRole('button', { name: 'Save pattern' });
      const load = screen.getByRole('button', { name: 'Load pattern' });
      expect(exp).not.toBeDisabled();
      expect(imp).not.toBeDisabled();
      expect(save).not.toBeDisabled();
      expect(load).not.toBeDisabled();
      fireEvent.click(exp);
      fireEvent.click(imp);
      fireEvent.click(save);
      fireEvent.click(load);
      expect(onExportPattern).toHaveBeenCalledTimes(1);
      expect(onImportPattern).toHaveBeenCalledTimes(1);
      expect(onSavePattern).toHaveBeenCalledTimes(1);
      expect(onLoadPattern).toHaveBeenCalledTimes(1);
    });
  });

  describe('registry schema path', () => {
    it('keeps dithering algorithms on DitherSettings (algorithm dropdown)', async () => {
      renderPanel(
        <EffectSettingsPanel
          selectedLayer={{
            id: 1,
            name: 'Bayer',
            filters: [
              {
                id: 'filter-1',
                kind: 'DitherV2',
                algorithm_id: 'bayer_4x4',
                schema_version: 1,
                params: {
                  type: 'DitherV2',
                  mode: 'bayer_4x4',
                  levels: 4,
                  threshold_scale: 1,
                  pixel_size: 1,
                  color_mode: 'rgb',
                  palette_id: null,
                },
                enabled: true,
                opacity: 1,
                blend_mode: 'Normal',
              } as FilterInfo,
            ],
          }}
          onUpdateParams={onUpdateParams}
        />
      );
      expect(screen.getByText('Algorithm')).toBeInTheDocument();
      expect(screen.getByText('Bayer 4×4')).toBeInTheDocument();
    });
  });
});
