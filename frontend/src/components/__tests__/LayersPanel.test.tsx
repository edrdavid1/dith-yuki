import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import LayersPanel from '../LayersPanel';
import type { LayersPanelProps } from '../LayersPanel';
import type { FilterInfo } from '../../types';
import type { LayerNodeDto } from '../../shared/types/layers';

const layer: LayerNodeDto = {
  id: 1,
  name: 'Image Source',
  kind: 'raster',
  blend_mode: 'Multiply',
  opacity: 0.5,
  visible: true,
};

const filter: FilterInfo = {
  id: 'filter-1',
  kind: 'DitherV2',
  params: {
    type: 'DitherV2',
    mode: 'floyd_steinberg',
    levels: 4,
    threshold_scale: 1,
    pixel_size: 1,
    color_mode: 'rgb',
    palette_id: null,
  } as FilterInfo['params'],
  enabled: true,
  opacity: 0.4,
  blend_mode: 'Overlay',
};

function renderLayers(overrides: Partial<LayersPanelProps> = {}) {
  const props: LayersPanelProps = {
    layers: [layer],
    selectedLayerId: 1,
    filters: [filter],
    selectedFilterId: null,
    onSelect: vi.fn(),
    onSelectFilter: vi.fn(),
    onAddLayer: vi.fn(),
    onRemoveFilter: vi.fn(),
    onReorderFilter: vi.fn(),
    onToggleVisibility: vi.fn(),
    onBlendModeChange: vi.fn(),
    onOpacityChange: vi.fn(),
    onFilterBlendChange: vi.fn(),
    ...overrides,
  };
  return { ...render(<LayersPanel {...props} />), props };
}

describe('LayersPanel blend/opacity', () => {
  it('edits the raster layer when Image Source is selected', () => {
    const { props } = renderLayers({ selectedFilterId: null, selectedLayerId: 1 });
    expect(screen.getByRole('button', { name: 'Multiply' })).toBeInTheDocument();
    expect(screen.getByLabelText('Opacity')).toHaveValue('50%');

    fireEvent.click(screen.getByRole('button', { name: 'Multiply' }));
    fireEvent.click(screen.getByRole('option', { name: 'Screen' }));
    expect(props.onBlendModeChange).toHaveBeenCalledWith(1, 'Screen');
    expect(props.onFilterBlendChange).not.toHaveBeenCalled();
  });

  it('edits the selected filter with the same controls', () => {
    const { props } = renderLayers({ selectedFilterId: 'filter-1', selectedLayerId: 1 });
    expect(screen.getByRole('button', { name: 'Overlay' })).toBeInTheDocument();
    expect(screen.getByLabelText('Opacity')).toHaveValue('40%');

    fireEvent.click(screen.getByRole('button', { name: 'Overlay' }));
    fireEvent.click(screen.getByRole('option', { name: 'Screen' }));
    expect(props.onFilterBlendChange).toHaveBeenCalledWith({ blend_mode: 'Screen' });
    expect(props.onBlendModeChange).not.toHaveBeenCalled();
  });
});
