import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import AlgorithmSettingsPanel from '../AlgorithmSettingsPanel';
import { getAlgorithmSchema } from '../../../shared/ipc/registry';
import type { ParamField } from '../../../shared/ipc/registry';

vi.mock('../../../shared/ipc/registry', () => ({
  getAlgorithmSchema: vi.fn(),
  listAlgorithmsForCategory: vi.fn(),
  EFFECT_CATEGORIES: [],
}));

const mockGetSchema = vi.mocked(getAlgorithmSchema);

const REGISTRY_IDS = [
  'adjust',
  'atkinson',
  'bayer_16x16',
  'bayer_2x2',
  'bayer_4x4',
  'bayer_8x8',
  'burkes',
  'clustered_dot_ordered',
  'cmyk_halftone',
  'crosshatch_dither',
  'crt',
  'curves',
  'dispersed_dot_ordered',
  'fan93',
  'floyd_steinberg',
  'glitch',
  'glow',
  'halftone_screen_angled',
  'jarvis_judice_ninke',
  'ostromoukhov',
  'palette_quantize',
  'riemersma',
  'shiau_fan',
  'sierra',
  'sierra_lite',
  'sierra_two_row',
  'stevenson_arce',
  'stucki',
  'void_and_cluster',
  'wave',
  'zhou_fang',
];

const SAMPLE_SCHEMA: ParamField[] = [
  {
    type: 'slider',
    key: 'levels',
    label: 'Levels',
    min: 2,
    max: 256,
    default: 4,
    step: null,
  },
  {
    type: 'checkbox',
    key: 'serpentine',
    label: 'Serpentine',
    default: false,
  },
  {
    type: 'dropdown',
    key: 'color_mode',
    label: 'Color Mode',
    options: [
      ['rgb', 'RGB'],
      ['grayscale', 'Grayscale'],
    ],
    default: 'rgb',
  },
];

describe('AlgorithmSettingsPanel', () => {
  beforeEach(() => {
    mockGetSchema.mockReset();
    mockGetSchema.mockResolvedValue(SAMPLE_SCHEMA);
  });

  it('shows a spinner while the schema loads', () => {
    mockGetSchema.mockReturnValue(new Promise(() => {}));
    render(
      <AlgorithmSettingsPanel algorithmId="bayer_4x4" values={{}} onChange={vi.fn()} />
    );
    expect(screen.getByRole('status')).toHaveTextContent('Loading…');
  });

  it('shows unknown effect when the schema request fails', async () => {
    mockGetSchema.mockRejectedValue(new Error('unknown algorithm: nope'));
    render(
      <AlgorithmSettingsPanel algorithmId="nope" values={{}} onChange={vi.fn()} />
    );
    await waitFor(() => {
      expect(screen.getByText('unknown effect')).toBeInTheDocument();
    });
  });

  it.each(REGISTRY_IDS)('renders without crashing for %s', async (id) => {
    render(
      <AlgorithmSettingsPanel
        algorithmId={id}
        values={{ levels: 4, serpentine: false, color_mode: 'rgb' }}
        onChange={vi.fn()}
      />
    );
    await waitFor(() => {
      expect(screen.getByText('Levels')).toBeInTheDocument();
    });
    expect(screen.getByText('Serpentine')).toBeInTheDocument();
    expect(screen.getByText('Color Mode')).toBeInTheDocument();
  });
});
