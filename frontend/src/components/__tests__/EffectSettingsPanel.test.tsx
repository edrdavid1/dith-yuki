import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import EffectSettingsPanel from '../EffectSettingsPanel';
import type { LayerWithFilters } from '../EffectSettingsPanel';
import type { FilterInfo } from '../../types';

// Mock listPalettes IPC call
vi.mock('../../ipc/commands', () => ({
  listPalettes: vi.fn().mockResolvedValue([]),
}));

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
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Algorithm')).toBeInTheDocument();
      const select = screen.getAllByRole('combobox').find(el => 
        el.querySelector('option[value="floyd_steinberg"]')
      );
      expect(select).toBeTruthy();
    });

    it('renders pixel size slider with range 1–32', () => {
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Pixel Size')).toBeInTheDocument();
      // Check that the slider element with min 1 and max 32 exists
      const sliders = document.querySelectorAll('input[type="range"]');
      const pixelSizeSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '1' && s.getAttribute('max') === '32'
      );
      expect(pixelSizeSlider).toBeTruthy();
    });

    it('renders threshold scale slider with range 0.1–4.0', () => {
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Threshold Scale')).toBeInTheDocument();
      const sliders = document.querySelectorAll('input[type="range"]');
      const tsSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '0.1' && s.getAttribute('max') === '4'
      );
      expect(tsSlider).toBeTruthy();
    });

    it('renders levels slider with range 2–256', () => {
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Levels')).toBeInTheDocument();
      const sliders = document.querySelectorAll('input[type="range"]');
      const levelsSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '2' && s.getAttribute('max') === '256'
      );
      expect(levelsSlider).toBeTruthy();
    });

    it('calls onUpdateParams with clamped pixel size', () => {
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const sliders = document.querySelectorAll('input[type="range"]');
      const pixelSizeSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '1' && s.getAttribute('max') === '32'
      ) as HTMLInputElement;
      
      fireEvent.change(pixelSizeSlider, { target: { value: '16' } });
      expect(onUpdateParams).toHaveBeenCalledWith(1, 'filter-1', expect.objectContaining({ pixel_size: 16 }));
    });

    it('calls onUpdateParams when algorithm changes', () => {
      render(<EffectSettingsPanel selectedLayer={makeDitherLayer()} onUpdateParams={onUpdateParams} />);
      const selects = screen.getAllByRole('combobox');
      const algorithmSelect = selects.find(s => 
        s.querySelector('option[value="bayer_2x2"]')
      ) as HTMLSelectElement;
      
      fireEvent.change(algorithmSelect, { target: { value: 'bayer_4x4' } });
      expect(onUpdateParams).toHaveBeenCalledWith(1, 'filter-1', expect.objectContaining({ mode: 'bayer_4x4' }));
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
      const sliders = document.querySelectorAll('input[type="range"]');
      const intensitySlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '0' && s.getAttribute('max') === '1'
      );
      expect(intensitySlider).toBeTruthy();
    });

    it('renders seed number input', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Seed')).toBeInTheDocument();
      const seedInput = document.querySelector('input[type="number"]') as HTMLInputElement;
      expect(seedInput).toBeTruthy();
      expect(seedInput.value).toBe('0');
    });

    it('clamps seed to 0–99999 range', () => {
      render(<EffectSettingsPanel selectedLayer={makeGlitchLayer()} onUpdateParams={onUpdateParams} />);
      const seedInput = document.querySelector('input[type="number"]') as HTMLInputElement;
      fireEvent.change(seedInput, { target: { value: '150000' } });
      expect(onUpdateParams).toHaveBeenCalledWith(2, 'filter-2', expect.objectContaining({ seed: 99999 }));
    });
  });

  describe('Curves settings', () => {
    it('renders channel dropdown', () => {
      render(<EffectSettingsPanel selectedLayer={makeCurvesLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Channel')).toBeInTheDocument();
      const select = screen.getByRole('combobox');
      expect(select).toHaveValue('All');
    });

    it('shows curve points editor', () => {
      render(<EffectSettingsPanel selectedLayer={makeCurvesLayer()} onUpdateParams={onUpdateParams} />);
      expect(screen.getByText('Curve Points')).toBeInTheDocument();
    });

    it('calls onUpdateParams when channel changes', () => {
      render(<EffectSettingsPanel selectedLayer={makeCurvesLayer()} onUpdateParams={onUpdateParams} />);
      const select = screen.getByRole('combobox');
      fireEvent.change(select, { target: { value: 'Red' } });
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
      const sliders = document.querySelectorAll('input[type="range"]');
      const gammaSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '0.1' && s.getAttribute('max') === '10'
      );
      expect(gammaSlider).toBeTruthy();
    });

    it('calls onUpdateParams with updated gamma', () => {
      render(<EffectSettingsPanel selectedLayer={makeRGBLayer()} onUpdateParams={onUpdateParams} />);
      const sliders = document.querySelectorAll('input[type="range"]');
      const gammaSlider = Array.from(sliders).find(
        s => s.getAttribute('min') === '0.1' && s.getAttribute('max') === '10'
      ) as HTMLInputElement;
      
      fireEvent.change(gammaSlider, { target: { value: '2.5' } });
      expect(onUpdateParams).toHaveBeenCalledWith(4, 'filter-4', expect.objectContaining({ gamma: 2.5 }));
    });
  });
});
