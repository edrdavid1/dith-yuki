import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import DitherSettings from '../DitherSettings';
import { ShellContext } from '../../../../app/shell/ShellContext';
import { StoreProvider } from '../../../../app/__tests__/testStore';

vi.mock('../../../../shared/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../../shared/ipc')>();
  return {
    ...actual,
    openDialog: vi.fn().mockResolvedValue(null),
  };
});

function renderSettings(ui: ReactElement) {
  return render(<StoreProvider>{ui}</StoreProvider>);
}

function openLabeledDropdown(ariaLabel: string) {
  fireEvent.click(screen.getByLabelText(ariaLabel));
}

describe('DitherSettings algorithm selector', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    warnSpy.mockRestore();
  });

  it('existing_algorithm_field_initializes_selector', () => {
    renderSettings(
      <DitherSettings
        params={{ mode: 'floyd_steinberg', levels: 4, threshold_scale: 1, pixel_size: 1 }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByText('Error Diffusion — CPU')).toBeInTheDocument();
    expect(screen.getByText('Floyd-Steinberg')).toBeInTheDocument();
  });

  it('unknown_initial_algorithm_falls_back', () => {
    renderSettings(
      <DitherSettings
        params={{ mode: 'banana', levels: 4, threshold_scale: 1, pixel_size: 1 }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByText('Error Diffusion — CPU')).toBeInTheDocument();
    expect(screen.getByText('Floyd-Steinberg')).toBeInTheDocument();
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('unknown mode "banana"'),
    );
  });

  it('other_controls_unchanged', () => {
    renderSettings(
      <DitherSettings
        params={{
          mode: 'floyd_steinberg',
          levels: 4,
          threshold_scale: 1,
          pixel_size: 1,
          dither_alpha: true,
        }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByText('Palette dither')).toBeInTheDocument();
    expect(screen.getByText('Match by brightness')).toBeInTheDocument();
    expect(screen.getByText('Pixel Size')).toBeInTheDocument();
    expect(screen.getByText('Pixelate Alpha')).toBeInTheDocument();
    expect(screen.getByText('Threshold Scale')).toBeInTheDocument();
    expect(screen.getByText('Tone Steps')).toBeInTheDocument();
  });

  it('banner_text_unchanged', () => {
    renderSettings(
      <DitherSettings
        params={{ mode: 'floyd_steinberg', levels: 4, threshold_scale: 1, pixel_size: 1 }}
        onUpdate={vi.fn()}
      />,
    );
    expect(
      screen.getByText(/Palette is controlled in Color Lab/, { exact: false }),
    ).toBeInTheDocument();
    const banner = screen.getByText(/Palette is controlled in Color Lab/);
    expect(banner.textContent).toMatch(/^Palette is controlled in Color Lab/);
  });

  it('hides the palette banner when tooltips are off', () => {
    render(
      <ShellContext.Provider
        value={{ tooltipsEnabled: false } as React.ContextType<typeof ShellContext>}
      >
        <StoreProvider>
          <DitherSettings
            params={{ mode: 'floyd_steinberg', levels: 4, threshold_scale: 1, pixel_size: 1 }}
            onUpdate={vi.fn()}
          />
        </StoreProvider>
      </ShellContext.Provider>,
    );
    expect(screen.queryByText(/Palette is controlled in Color Lab/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Tone Steps slider below/)).not.toBeInTheDocument();
    expect(screen.getByText('Palette dither')).toBeInTheDocument();
    expect(screen.getByText('Tone Steps')).toBeInTheDocument();
  });

  it('selector change updates mode via onUpdate', () => {
    const onUpdate = vi.fn();
    renderSettings(
      <DitherSettings
        params={{ mode: 'floyd_steinberg', levels: 4, threshold_scale: 1, pixel_size: 1 }}
        onUpdate={onUpdate}
      />,
    );
    openLabeledDropdown('Dither algorithm variant');
    const list = screen.getByRole('listbox');
    fireEvent.click(within(list).getByText('Atkinson'));
    expect(onUpdate).toHaveBeenCalled();
    const payload = onUpdate.mock.calls[0][0] as { mode: string };
    expect(payload.mode).toBe('atkinson');
  });

  it('shows Batch E knobs for crosshatch / line / voronoi / random', () => {
    const { rerender } = renderSettings(
      <DitherSettings
        params={{
          mode: 'crosshatch_dither',
          threshold_scale: 1,
          pixel_size: 1,
          wave_wavelength: 8,
          pattern_angle: 45,
        }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByText('Spacing')).toBeInTheDocument();
    expect(screen.getByText('Base Angle')).toBeInTheDocument();
    expect(screen.getByText('Line Thickness')).toBeInTheDocument();

    rerender(
      <StoreProvider>
        <DitherSettings
          params={{
            mode: 'line_screen',
            threshold_scale: 1,
            pixel_size: 1,
            halftone_cell_size: 8,
            pattern_angle: 45,
          }}
          onUpdate={vi.fn()}
        />
      </StoreProvider>,
    );
    expect(screen.getByText('Pitch')).toBeInTheDocument();
    expect(screen.getByText('Angle')).toBeInTheDocument();

    rerender(
      <StoreProvider>
        <DitherSettings
          params={{
            mode: 'voronoi_stipple',
            threshold_scale: 1,
            pixel_size: 2,
            halftone_cell_size: 8,
          }}
          onUpdate={vi.fn()}
        />
      </StoreProvider>,
    );
    expect(screen.getByText('Spacing')).toBeInTheDocument();
    expect(screen.getByText('Dot Size')).toBeInTheDocument();
    expect(screen.getByText('Density')).toBeInTheDocument();

    rerender(
      <StoreProvider>
        <DitherSettings
          params={{
            mode: 'random_dot_stipple',
            threshold_scale: 1,
            pixel_size: 1,
          }}
          onUpdate={vi.fn()}
        />
      </StoreProvider>,
    );
    expect(screen.getByText('Dot Size')).toBeInTheDocument();
    expect(screen.getByText('Density')).toBeInTheDocument();
  });
});
