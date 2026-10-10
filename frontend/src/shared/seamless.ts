/**
 * Client-side seamless / tileable period checks for ordered dither (Phase 3.1).
 * Mirrors `engine_project::seamless` for Bayer / clustered / dispersed / void-and-cluster.
 */

export type SeamlessKind = 'exact_ordered' | 'approximate_only' | 'unknown';

export interface SeamlessReport {
  kind: SeamlessKind;
  periodX: number | null;
  periodY: number | null;
  widthOk: boolean;
  heightOk: boolean;
  angleBreaksPeriodicity: boolean;
  nearestWidth: [number, number] | null;
  nearestHeight: [number, number] | null;
}

const MATRIX_SIDE: Record<string, [number, number]> = {
  bayer_2x2: [2, 2],
  bayer_4x4: [4, 4],
  bayer_8x8: [8, 8],
  bayer_16x16: [16, 16],
  clustered_dot_ordered: [8, 8],
  dispersed_dot_ordered: [16, 16],
  void_and_cluster: [64, 64],
};

const ED_MODES = new Set([
  'floyd_steinberg',
  'atkinson',
  'jarvis_judice_ninke',
  'stucki',
  'burkes',
  'fan93',
  'ostromoukhov',
  'sierra',
  'sierra_lite',
  'sierra_two_row',
  'shiau_fan',
  'stevenson_arce',
  'zhou_fang',
  'riemersma',
]);

export function patternAngleBreaksPeriodicity(angleDeg: number): boolean {
  if (!Number.isFinite(angleDeg)) return true;
  let a = angleDeg % 360;
  if (a < 0) a += 360;
  const nearest = Math.round(a);
  if (Math.abs(a - nearest) < 1e-3) a = nearest;
  const m = a % 90;
  return m > 1e-3 && 90 - m > 1e-3;
}

export function nearestMultiples(size: number, period: number): [number, number] | null {
  if (period <= 0) return null;
  if (size % period === 0) return [size, size];
  let below = Math.floor(size / period) * period;
  if (below === 0) below = period;
  return [below, below + period];
}

function modeKey(mode: unknown): string {
  if (typeof mode === 'string') return mode;
  if (typeof mode === 'object' && mode !== null && 'custom_png' in mode) return 'custom_png';
  return '';
}

export function checkSeamless(args: {
  mode: unknown;
  pixelSize: number;
  patternAngle: number;
  width: number;
  height: number;
  customMapSize?: [number, number] | null;
}): SeamlessReport {
  const key = modeKey(args.mode);
  if (ED_MODES.has(key)) {
    return {
      kind: 'approximate_only',
      periodX: null,
      periodY: null,
      widthOk: false,
      heightOk: false,
      angleBreaksPeriodicity: false,
      nearestWidth: null,
      nearestHeight: null,
    };
  }

  const sides = MATRIX_SIDE[key] ?? args.customMapSize ?? null;
  const angleBad = patternAngleBreaksPeriodicity(args.patternAngle);
  if (!sides) {
    return {
      kind: 'unknown',
      periodX: null,
      periodY: null,
      widthOk: false,
      heightOk: false,
      angleBreaksPeriodicity: angleBad,
      nearestWidth: null,
      nearestHeight: null,
    };
  }

  const ps = Math.max(1, Math.round(args.pixelSize) || 1);
  const periodX = sides[0] * ps;
  const periodY = sides[1] * ps;
  const widthOk = periodX > 0 && args.width % periodX === 0 && !angleBad;
  const heightOk = periodY > 0 && args.height % periodY === 0 && !angleBad;

  return {
    kind: 'exact_ordered',
    periodX,
    periodY,
    widthOk,
    heightOk,
    angleBreaksPeriodicity: angleBad,
    nearestWidth: nearestMultiples(args.width, periodX),
    nearestHeight: nearestMultiples(args.height, periodY),
  };
}

export function seamlessWarningLines(
  report: SeamlessReport,
  width: number,
  height: number
): string[] {
  if (report.kind === 'approximate_only') {
    return [
      'Error diffusion cannot be exactly seamless (sequential). Enable Approximate wrap-padding for a statistical edge fix — results are approximate only, not bit-periodic.',
    ];
  }
  if (report.kind === 'unknown') {
    const lines = [
      'Seamless period unknown for this mode (or custom map size missing).',
    ];
    if (report.angleBreaksPeriodicity) {
      lines.push('Pattern angle is not 0°/90° — the lattice is not axis-periodic.');
    }
    return lines;
  }
  const lines: string[] = [];
  if (report.angleBreaksPeriodicity) {
    lines.push('Pattern angle is not 0°/90° — tiling will show a dither seam.');
  }
  if (
    report.periodX != null &&
    report.periodY != null &&
    (!report.widthOk || !report.heightOk)
  ) {
    let msg = `Size ${width}×${height} is not a multiple of the pattern period ${report.periodX}×${report.periodY} px — a seam will appear at the tile edge.`;
    if (report.nearestWidth && report.nearestHeight) {
      const [wb, wa] = report.nearestWidth;
      const [hb, ha] = report.nearestHeight;
      msg += ` Nearest sizes: width ${wb}/${wa}, height ${hb}/${ha}.`;
    }
    lines.push(msg);
  }
  return lines;
}
