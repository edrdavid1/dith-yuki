import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import PaletteVolumeViewer from '../PaletteVolumeViewer';
import { createColorEntry } from '../types';

vi.mock('../../../shared/ipc', async () => {
  const actual = await vi.importActual<typeof import('../../../shared/ipc')>(
    '../../../shared/ipc'
  );
  return {
    ...actual,
    colorsToOklab: vi.fn().mockResolvedValue([]),
    logIpcError: vi.fn(),
  };
});

describe('PaletteVolumeViewer', () => {
  it('is hidden when the draft has no valid colors', () => {
    const { container } = render(
      <PaletteVolumeViewer
        colors={[]}
        selectedIndex={null}
        onSelectIndex={vi.fn()}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the oklab volume section when colors exist', () => {
    render(
      <PaletteVolumeViewer
        colors={[createColorEntry('#0f380f'), createColorEntry('#ffffff')]}
        selectedIndex={null}
        onSelectIndex={vi.fn()}
      />
    );
    expect(screen.getByText('oklab volume')).toBeInTheDocument();
    expect(screen.getByLabelText('Oklab palette volume')).toBeInTheDocument();
  });
});
