import { describe, expect, it } from 'vitest';
import { parseRuntimeDocId } from '../documentSlice';

describe('parseRuntimeDocId', () => {
  it('accepts positive numbers', () => {
    expect(parseRuntimeDocId(1)).toBe(1);
    expect(parseRuntimeDocId(42)).toBe(42);
  });

  it('rejects zero and negatives', () => {
    expect(parseRuntimeDocId(0)).toBeNull();
    expect(parseRuntimeDocId(-1)).toBeNull();
  });

  it('parses numeric strings and newtype-shaped objects', () => {
    expect(parseRuntimeDocId('3')).toBe(3);
    expect(parseRuntimeDocId({ 0: 7 })).toBe(7);
    expect(parseRuntimeDocId({ inner: 9 })).toBe(9);
  });

  it('rejects garbage', () => {
    expect(parseRuntimeDocId(null)).toBeNull();
    expect(parseRuntimeDocId(undefined)).toBeNull();
    expect(parseRuntimeDocId('x')).toBeNull();
    expect(parseRuntimeDocId({})).toBeNull();
  });
});
