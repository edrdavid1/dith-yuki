/**
 * Single source of truth for the dither algorithm family + variant selector UI.
 *
 * Adapted from `.local-doc/chose-algorithm.md` §3 for this app:
 * - Variant ids are the existing `params.mode` string ids (e.g. `jarvis_judice_ninke`).
 * - Families include the full current algorithm catalog so the UI does not regress.
 * - `gpu_ed` stays stubbed (`available: false`) until a capability probe exists.
 */

export type AlgorithmFamilyId = 'cpu_ed' | 'gpu_ed' | 'ordered';

export type AlgorithmVariantId =
  // cpu_ed
  | 'floyd_steinberg'
  | 'atkinson'
  | 'jarvis_judice_ninke'
  | 'stucki'
  | 'burkes'
  | 'fan93'
  | 'ostromoukhov'
  | 'sierra'
  | 'sierra_lite'
  | 'sierra_two_row'
  | 'shiau_fan'
  | 'stevenson_arce'
  | 'zhou_fang'
  | 'riemersma'
  // gpu_ed (stub — not valid DitherModeV2 yet)
  | 'approximate_fs'
  | 'stochastic_fs'
  // ordered / matrix / pattern
  | 'bayer_2x2'
  | 'bayer_4x4'
  | 'bayer_8x8'
  | 'bayer_16x16'
  | 'clustered_dot_ordered'
  | 'dispersed_dot_ordered'
  | 'void_and_cluster'
  | 'crosshatch_dither'
  | 'line_screen'
  | 'voronoi_stipple'
  | 'random_dot_stipple'
  | 'cmyk_halftone'
  | 'halftone_screen_angled'
  | 'wave'
  | 'custom_png';

export interface AlgorithmVariant {
  id: AlgorithmVariantId;
  label: string;
  tooltip?: string;
}

export interface AlgorithmFamily {
  id: AlgorithmFamilyId;
  label: string;
  tradeoff: string;
  tooltip: string;
  variants: AlgorithmVariant[];
  defaultVariant: AlgorithmVariantId;
  available: boolean;
}

export interface AlgorithmSelectorValue {
  family: AlgorithmFamilyId;
  variant: AlgorithmVariantId;
}

export const ALGORITHM_FAMILIES: AlgorithmFamily[] = [
  {
    id: 'cpu_ed',
    label: 'Error Diffusion — CPU',
    tradeoff: 'exact, deterministic, slower',
    tooltip: 'Exact error diffusion. Bit-identical across platforms. Slower on large images.',
    defaultVariant: 'floyd_steinberg',
    available: true,
    variants: [
      { id: 'floyd_steinberg', label: 'Floyd-Steinberg' },
      { id: 'atkinson', label: 'Atkinson' },
      { id: 'jarvis_judice_ninke', label: 'Jarvis-Judice-Ninke' },
      { id: 'stucki', label: 'Stucki' },
      { id: 'burkes', label: 'Burkes' },
      { id: 'fan93', label: 'Fan 93' },
      { id: 'ostromoukhov', label: 'Ostromoukhov' },
      { id: 'sierra', label: 'Sierra' },
      { id: 'sierra_lite', label: 'Sierra Lite' },
      { id: 'sierra_two_row', label: 'Sierra Two-Row' },
      { id: 'shiau_fan', label: 'Shiau–Fan' },
      { id: 'stevenson_arce', label: 'Stevenson–Arce' },
      { id: 'zhou_fang', label: 'Zhou–Fang' },
      { id: 'riemersma', label: 'Riemersma' },
    ],
  },
  {
    id: 'gpu_ed',
    label: 'Error Diffusion — GPU',
    tradeoff: 'fast, approximate',
    tooltip: 'GPU error diffusion. Fast on large images. Not bit-identical to CPU.',
    defaultVariant: 'approximate_fs',
    // TODO(chose-algorithm §3 / follow-up §11.2): replace stub with GPU capability probe.
    available: false,
    variants: [
      { id: 'approximate_fs', label: 'Approximate FS' },
      { id: 'stochastic_fs', label: 'Stochastic FS' },
    ],
  },
  {
    id: 'ordered',
    label: 'Ordered',
    tradeoff: 'fast, pixel-perfect, regular pattern',
    tooltip: 'Ordered dithering. Fast and deterministic. Regular pattern, animation-safe.',
    defaultVariant: 'bayer_8x8',
    available: true,
    variants: [
      { id: 'bayer_2x2', label: 'Bayer 2×2' },
      { id: 'bayer_4x4', label: 'Bayer 4×4' },
      { id: 'bayer_8x8', label: 'Bayer 8×8' },
      { id: 'bayer_16x16', label: 'Bayer 16×16' },
      { id: 'clustered_dot_ordered', label: 'Clustered Dot' },
      { id: 'dispersed_dot_ordered', label: 'Dispersed Dot' },
      { id: 'void_and_cluster', label: 'Void and Cluster' },
      { id: 'crosshatch_dither', label: 'Crosshatch' },
      { id: 'line_screen', label: 'Line Screen' },
      { id: 'voronoi_stipple', label: 'Voronoi Stipple' },
      { id: 'random_dot_stipple', label: 'Random Dot Stipple' },
      { id: 'cmyk_halftone', label: 'CMYK Halftone' },
      { id: 'halftone_screen_angled', label: 'Halftone Screen (Angled)' },
      { id: 'wave', label: 'Wave' },
      { id: 'custom_png', label: 'Custom Threshold Map' },
    ],
  },
];

const DEFAULT_SELECTION: AlgorithmSelectorValue = {
  family: 'cpu_ed',
  variant: 'floyd_steinberg',
};

export function getAlgorithmFamily(id: AlgorithmFamilyId): AlgorithmFamily | undefined {
  return ALGORITHM_FAMILIES.find((family) => family.id === id);
}

/** Map an existing `params.mode` string to a selector pair. */
export function selectionFromMode(mode: string): AlgorithmSelectorValue {
  for (const family of ALGORITHM_FAMILIES) {
    if (family.variants.some((variant) => variant.id === mode)) {
      return { family: family.id, variant: mode as AlgorithmVariantId };
    }
  }
  if (import.meta.env.DEV) {
    console.warn(
      `[dither_algorithm_selector_v1] unknown mode "${mode}", falling back to cpu_ed/floyd_steinberg`,
    );
  }
  return DEFAULT_SELECTION;
}

export function variantInFamily(
  family: AlgorithmFamily,
  variant: AlgorithmVariantId | undefined,
): AlgorithmVariantId {
  if (variant && family.variants.some((entry) => entry.id === variant)) {
    return variant;
  }
  return family.defaultVariant;
}
