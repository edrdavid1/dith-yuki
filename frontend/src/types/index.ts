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
}

export type FilterKind = 'Dither' | 'Curves' | 'Levels' | 'Glitch';

export type FilterParams =
  | DitherParams
  | CurvesParams
  | LevelsParams
  | GlitchParams;

export interface DitherParams {
  type: 'Dither';
  algorithm: DitherAlgorithm;
  color_depth: number;
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

export type DitherAlgorithm = 'FloydSteinberg' | 'Ordered' | 'Threshold';
export type CurveChannel = 'Red' | 'Green' | 'Blue' | 'All' | 'Luminance';
export type GlitchType = 'RGBShift' | 'BlockDisplace';

export interface ExportImageRequest {
  doc_id: number;
  path: string;
  format: 'PNG' | 'JPEG';
  quality?: number;
}
