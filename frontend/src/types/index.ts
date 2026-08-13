export interface LoadImageResponse {
  doc_id: number;
  width: number;
  height: number;
  tile_count: number;
}

export interface FilterInfo {
  id: string;
  kind: FilterKind;
  params: FilterParams;
  enabled: boolean;
  opacity: number;
  blend_mode: string;
}

export type FilterKind =
  | 'Dither'
  | 'DitherV2'
  | 'Curves'
  | 'Levels'
  | 'Glitch'
  | 'PaletteQuantize'
  | 'Glow'
  | 'Crt';

export type FilterParams =
  | DitherParams
  | DitherParamsV2
  | CurvesParams
  | LevelsParams
  | GlitchParams
  | PaletteQuantizeParams
  | GlowParams
  | CrtParams;

export interface DitherParams {
  type: 'Dither';
  mode: string;          // "ErrorDiffusion" | "Bayer" | "ThresholdMap"
  kernel?: string;       // "FloydSteinberg" | "Atkinson" | "JarvisJudiceNinke" | "Stucki"
  matrix_size?: number;  // 2 | 4 | 8 (for Bayer)
  path?: string;         // file path (for ThresholdMap)
  color_depth: number;
  // Legacy field kept for backwards compat
  algorithm?: DitherAlgorithm;
}

// ============================================================================
// DitherV2 Types (Redesigned dither filter)
// ============================================================================

/** Dither mode for the V2 redesigned filter. */
export type DitherModeV2 =
  | 'bayer_2x2'
  | 'bayer_4x4'
  | 'bayer_8x8'
  | { custom_png: { path: string } }
  | 'floyd_steinberg'
  | 'atkinson'
  | 'jarvis_judice_ninke'
  | 'stucki'
  | 'burkes'
  | 'sierra'
  | 'cmyk_halftone'
  | 'wave';

/** Color processing mode for dithering. */
export type DitherColorMode = 'rgb' | 'grayscale';

/** Full dither filter parameters (V2 redesign). */
export interface DitherParamsV2 {
  type: 'DitherV2';
  mode: DitherModeV2;
  levels: number;             // 2–256
  threshold_scale: number;    // 0.1–4.0, default 1.0
  pixel_size: number;         // 1–32, default 1
  color_mode: DitherColorMode;
  palette_id: number | null;
  /** CMYK halftone cell size (2–64), default 8 */
  halftone_cell_size?: number;
  /** Wave wavelength in px (2–256), default 8 */
  wave_wavelength?: number;
  /** Wave amplitude (0–1), default 1 */
  wave_amplitude?: number;
  /** Wave phase in radians */
  wave_phase?: number;
  /** Wave angle in degrees */
  wave_angle?: number;
  /** Ordered-threshold shift [-0.5, 0.5], default 0. Ignored by ED modes. */
  threshold_bias?: number;
  /** Bayer / CustomPng pattern sampling angle in degrees, default 0. */
  pattern_angle?: number;
  /** ED serpentine scan (odd global rows R→L). Default false. */
  serpentine?: boolean;
}

export interface CurvesParams {
  type: 'Curves';
  curve: [number, number][];
  channel: CurveChannel;
}

export interface LevelsParams {
  type: 'Levels';
  input_black: number;
  input_white: number;
  gamma: number;
  output_black: number;
  output_white: number;
}

export interface GlitchParams {
  type: 'Glitch';
  glitch_type: GlitchType;
  intensity: number;
  seed: number;
}

export interface PaletteQuantizeParams {
  type: 'PaletteQuantize';
  palette_id: number;
  diffusion: string | null; // "FloydSteinberg" | "Atkinson" | "JarvisJudiceNinke" | "Stucki" | null
}

export interface GlowParams {
  type: 'Glow';
  radius: number;      // 0.5–2 (HALO cap)
  intensity: number;   // 0–4
  threshold: number;   // 0–1
}

export interface CrtParams {
  type: 'Crt';
  period: number;        // 2–8
  strength: number;      // 0–1
  mask_strength: number; // 0–1
}

export type DitherAlgorithm = 'FloydSteinberg' | 'Ordered' | 'Threshold';
export type DitherMode = 'ErrorDiffusion' | 'Bayer' | 'ThresholdMap';
export type DiffusionKernel = 'FloydSteinberg' | 'Atkinson' | 'JarvisJudiceNinke' | 'Stucki';
export type CurveChannel = 'Red' | 'Green' | 'Blue' | 'All' | 'Luminance';
export type GlitchType = 'RGBShift' | 'BlockDisplace';

export interface ExportImageRequest {
  doc_id: number;
  path: string;
  format: 'PNG' | 'JPEG' | 'SVG';
  quality?: number;
}
