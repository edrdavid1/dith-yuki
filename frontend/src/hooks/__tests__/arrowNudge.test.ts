import { describe, expect, it } from 'vitest';
import {
  decimalPlacesFromText,
  formatAtPrecision,
  nudgeByDisplayPrecision,
  roundToDecimalPlaces,
} from '../arrowNudge';

describe('decimalPlacesFromText', () => {
  it('counts fractional digits; integer → 0', () => {
    expect(decimalPlacesFromText('42')).toBe(0);
    expect(decimalPlacesFromText('3.7')).toBe(1);
    expect(decimalPlacesFromText('0.25')).toBe(2);
    expect(decimalPlacesFromText('0.001')).toBe(3);
  });

  it('handles trailing dot, leading minus, percent', () => {
    expect(decimalPlacesFromText('1.')).toBe(0);
    expect(decimalPlacesFromText('-3.7')).toBe(1);
    expect(decimalPlacesFromText(' 42% ')).toBe(0);
  });

  it('rejects empty / non-numeric', () => {
    expect(decimalPlacesFromText('')).toBeNull();
    expect(decimalPlacesFromText('-')).toBeNull();
    expect(decimalPlacesFromText('abc')).toBeNull();
    expect(decimalPlacesFromText('1.2.3')).toBeNull();
  });
});

describe('roundToDecimalPlaces', () => {
  it('avoids float artifacts', () => {
    expect(roundToDecimalPlaces(0.1 + 0.2, 1)).toBe(0.3);
    expect(roundToDecimalPlaces(0.1 + 0.2, 2)).toBe(0.3);
  });
});

describe('nudgeByDisplayPrecision', () => {
  it('steps integers by 1', () => {
    expect(nudgeByDisplayPrecision('42', 1, 0, 100)).toEqual({
      value: 43,
      text: '43',
      decimals: 0,
      step: 1,
    });
    expect(nudgeByDisplayPrecision('42', -1, 0, 100)?.value).toBe(41);
  });

  it('steps by 10^-n for fractions', () => {
    expect(nudgeByDisplayPrecision('3.7', 1, 0, 10)?.text).toBe('3.8');
    expect(nudgeByDisplayPrecision('0.25', -1, 0, 1)?.text).toBe('0.24');
    expect(nudgeByDisplayPrecision('0.001', 1, 0, 1)?.text).toBe('0.002');
  });

  it('clamps to min/max', () => {
    expect(nudgeByDisplayPrecision('0', -1, 0, 10)?.value).toBe(0);
    expect(nudgeByDisplayPrecision('10', 1, 0, 10)?.value).toBe(10);
  });

  it('returns null for invalid text', () => {
    expect(nudgeByDisplayPrecision('', 1, 0, 10)).toBeNull();
    expect(nudgeByDisplayPrecision('abc', 1, 0, 10)).toBeNull();
    expect(nudgeByDisplayPrecision('1.', 1, 0, 10)?.value).toBe(2);
  });

  it('formats at the same precision', () => {
    expect(formatAtPrecision(3, 2)).toBe('3.00');
    expect(nudgeByDisplayPrecision('3.70', 1, 0, 10)?.text).toBe('3.71');
  });
});
