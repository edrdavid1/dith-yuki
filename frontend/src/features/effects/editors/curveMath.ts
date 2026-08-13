/** Control point in normalized [0, 1] space (input, output). */
export type CurvePoint = [number, number];

/** Minimum X separation so Catmull-Rom segments never degenerate. ~1 level in 8-bit. */
export const MIN_POINT_GAP = 1 / 255;

const ENDPOINT_EPS = 0.001;

export function clamp01(v: number): number {
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

/** Round-trip with the 0–255 Input/Output fields (Photoshop convention). */
export function toByte(v: number): number {
  return Math.round(clamp01(v) * 255);
}

export function fromByte(v: number): number {
  return clamp01(v / 255);
}

/**
 * Catmull-Rom evaluation matching `CurvesFilter::evaluate` in engine-project.
 * Output is clamped to [0, 1].
 */
export function evaluateCurve(curve: CurvePoint[], x: number): number {
  if (curve.length === 0) return clamp01(x);
  if (curve.length === 1) return clamp01(curve[0][1]);

  const tIn = clamp01(x);

  if (Math.abs(tIn - 0) < ENDPOINT_EPS) {
    return clamp01(curve[0][1]);
  }
  if (Math.abs(tIn - 1) < ENDPOINT_EPS) {
    return clamp01(curve[curve.length - 1][1]);
  }

  let i = 0;
  while (i < curve.length - 1 && curve[i + 1][0] <= tIn) {
    i += 1;
  }
  if (i >= curve.length - 1) {
    return clamp01(curve[curve.length - 1][1]);
  }

  const p1 = curve[i];
  const p2 = curve[i + 1];
  const span = p2[0] - p1[0];
  if (span <= 0) {
    return clamp01(p1[1]);
  }

  const p0: CurvePoint = i === 0
    ? [p1[0] - (p2[0] - p1[0]), p1[1]]
    : curve[i - 1];
  const p3: CurvePoint = i + 2 < curve.length
    ? curve[i + 2]
    : [
        curve[curve.length - 1][0]
          + (curve[curve.length - 1][0] - curve[curve.length - 2][0]),
        curve[curve.length - 1][1],
      ];

  const t = clamp01((tIn - p1[0]) / span);
  const t2 = t * t;
  const t3 = t2 * t;

  const a0 = -0.5 * p0[1] + 1.5 * p1[1] - 1.5 * p2[1] + 0.5 * p3[1];
  const a1 = p0[1] - 2.5 * p1[1] + 2.0 * p2[1] - 0.5 * p3[1];
  const a2 = -0.5 * p0[1] + 0.5 * p2[1];
  const a3 = p1[1];

  return clamp01(a0 * t3 + a1 * t2 + a2 * t + a3);
}

export function sampleCurve(curve: CurvePoint[], samples = 128): CurvePoint[] {
  const n = Math.max(2, samples);
  const out: CurvePoint[] = [];
  for (let i = 0; i < n; i++) {
    const x = i / (n - 1);
    out.push([x, evaluateCurve(curve, x)]);
  }
  return out;
}

export function curveToSvgPath(
  curve: CurvePoint[],
  size: number,
  pad: number,
  samples = 128,
): string {
  const inner = size - pad * 2;
  const pts = sampleCurve(curve, samples);
  return pts
    .map(([x, y], i) => {
      const px = pad + x * inner;
      const py = pad + (1 - y) * inner;
      return `${i === 0 ? 'M' : 'L'}${px.toFixed(2)} ${py.toFixed(2)}`;
    })
    .join(' ');
}

export function curveToPixel(
  x: number,
  y: number,
  size: number,
  pad: number,
): [number, number] {
  const inner = size - pad * 2;
  return [pad + clamp01(x) * inner, pad + (1 - clamp01(y)) * inner];
}

export function pixelToCurve(
  px: number,
  py: number,
  width: number,
  height: number,
  padFrac: number,
): [number, number] {
  const padX = padFrac * width;
  const padY = padFrac * height;
  const innerW = Math.max(1, width - padX * 2);
  const innerH = Math.max(1, height - padY * 2);
  return [(px - padX) / innerW, 1 - (py - padY) / innerH];
}

export function xBounds(curve: CurvePoint[], index: number): { min: number; max: number } {
  const prev = index > 0 ? curve[index - 1][0] + MIN_POINT_GAP : 0;
  const next = index < curve.length - 1 ? curve[index + 1][0] - MIN_POINT_GAP : 1;
  return { min: prev, max: Math.max(prev, next) };
}

export function movePoint(
  curve: CurvePoint[],
  index: number,
  x: number,
  y: number,
): CurvePoint[] {
  if (index < 0 || index >= curve.length) return curve;
  const { min, max } = xBounds(curve, index);
  const next = curve.map((p) => [p[0], p[1]] as CurvePoint);
  next[index] = [
    Math.min(max, Math.max(min, x)),
    clamp01(y),
  ];
  return next;
}

function separatedX(curve: CurvePoint[], x: number): number | null {
  const occupied = curve.map((p) => p[0]);
  const hits = (v: number) => occupied.some((ox) => Math.abs(ox - v) < MIN_POINT_GAP);
  let px = clamp01(x);
  if (!hits(px)) return px;
  for (let i = 1; i < 256; i++) {
    const right = px + i * MIN_POINT_GAP;
    if (right <= 1 && !hits(right)) return right;
    const left = px - i * MIN_POINT_GAP;
    if (left >= 0 && !hits(left)) return left;
  }
  return null;
}

export function addPoint(
  curve: CurvePoint[],
  x: number,
  y: number,
): { curve: CurvePoint[]; index: number } | null {
  const px = separatedX(curve, x);
  if (px === null) return null;
  const point: CurvePoint = [px, clamp01(y)];
  const next = [...curve, point].sort((a, b) => a[0] - b[0]);
  const index = next.findIndex((p) => p[0] === point[0] && p[1] === point[1]);
  return { curve: next, index: index < 0 ? 0 : index };
}

export function removePoint(curve: CurvePoint[], index: number): CurvePoint[] | null {
  if (curve.length <= 2) return null;
  if (index < 0 || index >= curve.length) return null;
  return curve.filter((_, i) => i !== index);
}

export function nearestPointIndex(
  curve: CurvePoint[],
  x: number,
  y: number,
  maxDist: number,
): number | null {
  let best = -1;
  let bestD = maxDist;
  for (let i = 0; i < curve.length; i++) {
    const dx = curve[i][0] - x;
    const dy = curve[i][1] - y;
    const d = Math.hypot(dx, dy);
    if (d <= bestD) {
      bestD = d;
      best = i;
    }
  }
  return best >= 0 ? best : null;
}

export const IDENTITY_CURVE: CurvePoint[] = [[0, 0], [1, 1]];
