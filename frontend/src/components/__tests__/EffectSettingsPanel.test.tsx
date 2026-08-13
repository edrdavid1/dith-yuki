import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { ReactElement } from 'react';
import EffectSettingsPanel from '../EffectSettingsPanel';
import type { LayerWithFilters } from '../EffectSettingsPanel';
import type { FilterInfo } from '../../types';
import { StoreProvider } from '../../app/__tests__/testStore';

// Mock listPalettes IPC call
vi.mock('../../ipc/commands', () => ({
  listPalettes: vi.fn().mockResolvedValue([]),
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
    it('shows effect chooser when no layer selected', () => {
      render(<EffectSettingsPanel selectedLayer={null} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Effect')).toBeInTheDocument();
      // Should show the 4 effect type options
      expect(screen.getByText('Dithering')).toBeInTheDocument();
      expect(screen.getByText('Glitching')).toBeInTheDocument();
      expect(screen.getByText('Curves')).toBeInTheDocument();
      expect(screen.getByText('RGB channels')).toBeInTheDocument();
      // Should not have any sliders
      expect(screen.queryByRole('slider')).not.toBeInTheDocument();
    });

    it('shows effect chooser when Image_Source_Layer is selected (no filters)', () => {
      render(<EffectSettingsPanel selectedLayer={makeImageSourceLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Effect')).toBeInTheDocument();
      expect(screen.getByText('Dithering')).toBeInTheDocument();
      expect(screen.queryByRole('slider')).not.toBeInTheDocument();
    });

    it('calls onSelectEffect when effect type is clicked', () => {
      const onSelectEffect = vi.fn();
      render(<EffectSettingsPanel selectedLayer={null} onUpdateParams={onUpdateParams} onSelectEffect={onSelectEffect} />);
      fireEvent.click(screen.getByText('Dithering'));
      expect(onSelectEffect).toHaveBeenCalledWith('Dithering');
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

    it('calls onUpdateParams with clamped pixel size', () => {
      renderPanel(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const input = sliderValueInput('Pixel Size');
      fireEvent.change(input, { target: { value: '16' } });
      fireEvent.blur(input);
      expect(onUpdateParams).toHaveBeenCalledWith(1, 'filter-1', expect.objectContaining({ pixel_size: 16 }));
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
      expect(screen.getByText('Export as pattern…')).toBeDisabled();
      expect(screen.getByText('Import pattern…')).toBeDisabled();
    });

    it('enables pattern actions when a layer is targeted', () => {
      const onExportPattern = vi.fn();
      const onImportPattern = vi.fn();
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
        />
      );
      const exp = screen.getByText('Export as pattern…');
      const imp = screen.getByText('Import pattern…');
      expect(exp).not.toBeDisabled();
      expect(imp).not.toBeDisabled();
      fireEvent.click(exp);
      fireEvent.click(imp);
      expect(onExportPattern).toHaveBeenCalledTimes(1);
      expect(onImportPattern).toHaveBeenCalledTimes(1);
    });
  });
});
