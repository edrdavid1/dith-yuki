/**
 * ArrowUp/ArrowDown step from the *displayed* field text:
 * step = 10^-n where n is fractional digit count (integer → 1).
 */

export function decimalPlacesFromText(raw: string): number | null {
  const s = raw.trim().replace(/%/g, '');
  if (s === '' || s === '-' || s === '+' || s === '.' || s === '-.' || s === '+.') {
    return null;
  }
  // Integers, "1.", "1.0", ".5", "-.25"
  if (!/^[+-]?(?:\d+\.?\d*|\.\d+)$/.test(s)) {
    return null;
  }
  const dot = s.indexOf('.');
  if (dot === -1) return 0;
  return s.length - dot - 1;
}

export function roundToDecimalPlaces(value: number, decimals: number): number {
  if (!Number.isFinite(value)) return value;
  if (decimals <= 0) return Math.round(value);
  // toFixed avoids binary float artifacts (e.g. 0.1 + 0.2).
  return Number(value.toFixed(decimals));
}

export function formatAtPrecision(value: number, decimals: number): string {
  if (decimals <= 0) return String(Math.round(value));
  return value.toFixed(decimals);
}

export type ArrowNudgeResult = {
  value: number;
  text: string;
  decimals: number;
  step: number;
};

/**
 * Nudge a field string by ArrowUp (+1) / ArrowDown (-1).
 * Returns null for empty / non-numeric text (caller still preventDefaults).
 */
export function nudgeByDisplayPrecision(
  raw: string,
  direction: 1 | -1,
  min: number,
  max: number
): ArrowNudgeResult | null {
  const decimals = decimalPlacesFromText(raw);
  if (decimals === null) return null;
  const parsed = parseFloat(raw.trim().replace(/%/g, ''));
  if (!Number.isFinite(parsed)) return null;

  const step = decimals === 0 ? 1 : 10 ** -decimals;
  const next = roundToDecimalPlaces(parsed + direction * step, decimals);
  const clamped = Math.min(max, Math.max(min, next));
  const value = roundToDecimalPlaces(clamped, decimals);
  return {
    value,
    text: formatAtPrecision(value, decimals),
    decimals,
    step,
  };
}
