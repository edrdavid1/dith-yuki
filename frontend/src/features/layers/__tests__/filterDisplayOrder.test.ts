import { describe, it, expect } from 'vitest';
import {
  displayFilterOrder,
  stackIndexAfterDisplayReorder,
} from '../filterDisplayOrder';

describe('filterDisplayOrder', () => {
  it('keeps document order (top row first)', () => {
    expect(displayFilterOrder(['dither', 'adjust'])).toEqual(['dither', 'adjust']);
  });

  it('maps drop at top of the panel to storage index 0', () => {
    expect(stackIndexAfterDisplayReorder(2, 1, 0)).toBe(0);
  });

  it('maps drop just above Image Source to the last storage index', () => {
    expect(stackIndexAfterDisplayReorder(2, 0, 2)).toBe(1);
  });
});
