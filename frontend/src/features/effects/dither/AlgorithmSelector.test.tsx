import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import { AlgorithmSelector } from './AlgorithmSelector';
import type { AlgorithmSelectorValue } from './algorithmFamilies';
import { setGpuDitherFamiliesAvailable } from './algorithmFamilies';
import * as WorkspaceContext from '../../workspace/WorkspaceContext';

function openLabeledDropdown(ariaLabel: string) {
  fireEvent.click(screen.getByLabelText(ariaLabel));
}

function selectOption(label: string) {
  const list = screen.getByRole('listbox');
  fireEvent.click(within(list).getByText(label));
}

describe('AlgorithmSelector', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setGpuDitherFamiliesAvailable(true);
  });

  it('renders_two_dropdowns', () => {
    render(
      <AlgorithmSelector
        value={{ family: 'cpu_ed', variant: 'floyd_steinberg' }}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText('Algorithm family')).toBeInTheDocument();
    expect(screen.getByText('Algorithm')).toBeInTheDocument();
    expect(screen.getByLabelText('Dither algorithm family')).toBeInTheDocument();
    expect(screen.getByLabelText('Dither algorithm variant')).toBeInTheDocument();
  });

  it('family_change_updates_variants', () => {
    const onChange = vi.fn();
    const value: AlgorithmSelectorValue = {
      family: 'cpu_ed',
      variant: 'floyd_steinberg',
    };
    const { rerender } = render(
      <AlgorithmSelector value={value} onChange={onChange} />,
    );

    openLabeledDropdown('Dither algorithm family');
    selectOption('Ordered');

    expect(onChange).toHaveBeenCalledWith({
      family: 'ordered',
      variant: 'bayer_8x8',
    });

    rerender(
      <AlgorithmSelector
        value={{ family: 'ordered', variant: 'bayer_8x8' }}
        onChange={onChange}
      />,
    );

    openLabeledDropdown('Dither algorithm variant');
    const list = screen.getByRole('listbox');
    expect(within(list).getByText('Bayer 2×2')).toBeInTheDocument();
    expect(within(list).getByText('Bayer 4×4')).toBeInTheDocument();
    expect(within(list).getByText('Bayer 8×8')).toBeInTheDocument();
    expect(within(list).queryByText('Floyd-Steinberg')).not.toBeInTheDocument();
  });

  it('variant_persists_per_family', () => {
    const onChange = vi.fn();
    let value: AlgorithmSelectorValue = {
      family: 'cpu_ed',
      variant: 'floyd_steinberg',
    };
    const { rerender } = render(
      <AlgorithmSelector
        value={value}
        onChange={(next) => {
          onChange(next);
          value = next;
        }}
      />,
    );

    openLabeledDropdown('Dither algorithm variant');
    selectOption('Jarvis-Judice-Ninke');
    expect(onChange).toHaveBeenLastCalledWith({
      family: 'cpu_ed',
      variant: 'jarvis_judice_ninke',
    });
    rerender(
      <AlgorithmSelector
        value={value}
        onChange={(next) => {
          onChange(next);
          value = next;
        }}
      />,
    );

    openLabeledDropdown('Dither algorithm family');
    selectOption('Ordered');
    rerender(
      <AlgorithmSelector
        value={value}
        onChange={(next) => {
          onChange(next);
          value = next;
        }}
      />,
    );

    openLabeledDropdown('Dither algorithm family');
    selectOption('Error Diffusion — CPU');
    expect(onChange).toHaveBeenLastCalledWith({
      family: 'cpu_ed',
      variant: 'jarvis_judice_ninke',
    });
  });

  it('unavailable_family_is_disabled', () => {
    vi.spyOn(WorkspaceContext, 'useWorkspaceOptional').mockReturnValue({
      gpuDitherFamilies: false,
    } as ReturnType<typeof WorkspaceContext.useWorkspaceOptional>);
    render(
      <AlgorithmSelector
        value={{ family: 'cpu_ed', variant: 'floyd_steinberg' }}
        onChange={vi.fn()}
      />,
    );
    openLabeledDropdown('Dither algorithm family');
    const list = screen.getByRole('listbox');
    const gpu = within(list).getByText('Error Diffusion — GPU (unavailable)');
    expect(gpu).toBeInTheDocument();
    expect(gpu.closest('[role="option"]')).toHaveAttribute('aria-disabled', 'true');
    expect(gpu.closest('[role="option"]')).toHaveAttribute(
      'title',
      'Disabled: experimental.gpu_dither_families is off in config.json',
    );
  });

  it('onchange_emits_full_pair', () => {
    const onChange = vi.fn();
    render(
      <AlgorithmSelector
        value={{ family: 'cpu_ed', variant: 'stucki' }}
        onChange={onChange}
      />,
    );
    openLabeledDropdown('Dither algorithm family');
    selectOption('Ordered');
    expect(onChange).toHaveBeenCalledWith({
      family: 'ordered',
      variant: 'bayer_8x8',
    });
  });

  it('disabled_prop_disables_both_dropdowns', () => {
    render(
      <AlgorithmSelector
        value={{ family: 'cpu_ed', variant: 'floyd_steinberg' }}
        onChange={vi.fn()}
        disabled
      />,
    );
    const family = screen.getByLabelText('Dither algorithm family');
    const variant = screen.getByLabelText('Dither algorithm variant');
    expect(family).toHaveClass('disabled');
    expect(variant).toHaveClass('disabled');
    expect(family).toHaveAttribute('tabIndex', '-1');
    expect(variant).toHaveAttribute('tabIndex', '-1');
  });

  it('tradeoff_line_updates_on_family_change', () => {
    const { rerender } = render(
      <AlgorithmSelector
        value={{ family: 'cpu_ed', variant: 'floyd_steinberg' }}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText('exact, deterministic, slower')).toBeInTheDocument();

    rerender(
      <AlgorithmSelector
        value={{ family: 'ordered', variant: 'bayer_8x8' }}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText('fast, pixel-perfect, regular pattern')).toBeInTheDocument();
  });
});
