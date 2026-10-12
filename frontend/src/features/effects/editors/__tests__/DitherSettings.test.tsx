import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import DitherSettings from '../DitherSettings';
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
    expect(screen.getByText('Levels')).toBeInTheDocument();
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
});
