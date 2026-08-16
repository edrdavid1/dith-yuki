import type { FilterKind } from './index';

// =============================================================================
// Effect Types and Mappings
// =============================================================================

/** Effect types available in the new design */
export type EffectType = 'Dithering' | 'Glitching' | 'Curves' | 'RGBChannels' | 'Glow' | 'CRT' | 'Adjust';

/** Maps EffectType to the corresponding FilterKind used in IPC */
export const EFFECT_TO_FILTER_KIND: Record<EffectType, FilterKind> = {
  Dithering: 'DitherV2',
  Glitching: 'Glitch',
  Curves: 'Curves',
  RGBChannels: 'Levels',
  Glow: 'Glow',
  CRT: 'Crt',
  Adjust: 'Adjust',
};

/** Default params for each effect type on creation */
export const EFFECT_DEFAULTS: Record<EffectType, Record<string, unknown>> = {
  Dithering: {
    mode: 'floyd_steinberg',
    levels: 4,
    threshold_scale: 1.0,
    pixel_size: 1,
    color_mode: 'rgb',
    palette_id: null,
    palette_dither_mode: 'strict',
    halftone_cell_size: 8,
    wave_wavelength: 8,
    wave_amplitude: 1,
    wave_phase: 0,
    wave_angle: 0,
    threshold_bias: 0,
    pattern_angle: 0,
    serpentine: false,
    dither_alpha: true,
  },
  Glitching: {
    glitch_type: 'RGBShift',
    intensity: 0.5,
    seed: 0,
  },
  Curves: {
    curve: [[0, 0], [1, 1]],
    channel: 'All',
  },
  RGBChannels: {
    input_black: 0.0,
    input_white: 1.0,
    gamma: 1.0,
    output_black: 0.0,
    output_white: 1.0,
    channel_r: true,
    channel_g: true,
    channel_b: true,
  },
  Glow: {
    radius: 2.0,
    intensity: 1.0,
    threshold: 0.0,
  },
  CRT: {
    period: 2,
    strength: 0.5,
    mask_strength: 0.0,
  },
  Adjust: {
    contrast: 0,
    brightness: 0,
    saturation: 0,
    blur: 0,
    sharpness: 0,
    noise: 0,
  },
};

// =============================================================================
// Zoom Model
// =============================================================================

/** Zoom preset steps for the preview window */
export const ZOOM_PRESETS = [25, 50, 100, 200, 400] as const;

/** Minimum zoom percentage */
export const ZOOM_MIN = 1;

/** Maximum zoom percentage */
export const ZOOM_MAX = 6400;

/**
 * Get the next zoom preset above the current value.
 * If current is above the max preset, doubles the value (capped at ZOOM_MAX).
 */
export function nextZoomPreset(current: number): number {
  const next = ZOOM_PRESETS.find(p => p > current);
  if (next) return next;
  return Math.min(current * 2, ZOOM_MAX);
}

/**
 * Get the previous zoom preset below the current value.
 * If current is below the min preset, halves the value (floored at ZOOM_MIN).
 */
export function prevZoomPreset(current: number): number {
  const prev = [...ZOOM_PRESETS].reverse().find(p => p < current);
  if (prev) return prev;
  return Math.max(current / 2, ZOOM_MIN);
}

// =============================================================================
// Parameter Utilities
// =============================================================================

/**
 * Clamp a numeric value to the range [min, max].
 */
export function clampParam(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

// =============================================================================
// Layer Display
// =============================================================================

/** Extended layer info for UI display */
export interface LayerDisplayInfo {
  id: number;
  name: string;
  effectType: EffectType | null; // null for Image_Source_Layer
  effectIcon: string;            // emoji/icon for the effect type
  visible: boolean;
  isImageSource: boolean;        // true if filters.length === 0
}

// =============================================================================
// Hex Color Utilities
// =============================================================================

/**
 * Returns true if the string is a valid hex color in #RRGGBB format.
 */
export function isValidHex(s: string): boolean {
  return /^#[0-9A-Fa-f]{6}$/.test(s);
}

/**
 * Parse a #RRGGBB hex string into an [r, g, b] tuple (0–255 each).
 * Assumes the input is a valid hex string.
 */
export function parseHex(s: string): [number, number, number] {
  const r = parseInt(s.slice(1, 3), 16);
  const g = parseInt(s.slice(3, 5), 16);
  const b = parseInt(s.slice(5, 7), 16);
  return [r, g, b];
}

/**
 * Convert r, g, b values (0–255) to a lowercase #rrggbb hex string.
 */
export function toHex(r: number, g: number, b: number): string {
  const hex = (v: number) => v.toString(16).padStart(2, '0');
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

// =============================================================================
// Sort by Brightness (Oklab L*)
// =============================================================================

/**
 * Convert linear sRGB (0–1) to Oklab L* component.
 * Uses the Oklab conversion via LMS intermediate.
 */
function srgbToLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function oklabL(r: number, g: number, b: number): number {
  const lr = srgbToLinear(r);
  const lg = srgbToLinear(g);
  const lb = srgbToLinear(b);

  // sRGB → LMS (approximate matrix from Oklab spec)
  const l = 0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb;
  const m = 0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb;
  const s = 0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb;

  // LMS → Oklab L*
  const l_ = Math.cbrt(l);
  const m_ = Math.cbrt(m);
  const s_ = Math.cbrt(s);

  return 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
}

/**
 * Sort an array of RGB colors by Oklab L* in non-decreasing order.
 * Each color is a tuple [r, g, b] with values in 0–255.
 */
export function sortByBrightness(
  colors: [number, number, number][]
): [number, number, number][] {
  return [...colors].sort((a, b) => {
    const la = oklabL(a[0], a[1], a[2]);
    const lb = oklabL(b[0], b[1], b[2]);
    return la - lb;
  });
}

// =============================================================================
// Document Structure Validation
// =============================================================================

/**
 * Validate that a document's layer tree has proper structure.
 * The first layer in the list is the image source layer — it can have any
 * number of filters (each filter is displayed as a virtual "effect layer" in the UI).
 * All other layers must have exactly 1 filter.
 *
 * Returns {valid: true} if valid, or {valid: false, layerId} for the first
 * offending layer.
 */
export function validateDocumentStructure(
  layers: { id: number; filters: unknown[] }[]
): { valid: true } | { valid: false; layerId: number } {
  for (let i = 0; i < layers.length; i++) {
    // First layer is image source — any filter count is valid
    if (i === 0) continue;
    const layer = layers[i];
    if (layer.filters.length !== 0 && layer.filters.length !== 1) {
      return { valid: false, layerId: layer.id };
    }
  }
  return { valid: true };
}
