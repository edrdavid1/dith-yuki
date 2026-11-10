import { describe, it, expect } from 'vitest';
import { classifyDropOutlineAxis } from '../dropOutlineAxis';

describe('classifyDropOutlineAxis', () => {
  const L = 332;
  const H = 800;

  it('labels a full-panel overlay as center (not side)', () => {
    expect(classifyDropOutlineAxis(L, H, L, H)).toBe('center');
    expect(classifyDropOutlineAxis(L * 0.9, H * 0.9, L, H)).toBe('center');
  });

  it('labels a wide short band as stack', () => {
    expect(classifyDropOutlineAxis(L, H * 0.5, L, H)).toBe('stack');
  });

  it('labels a tall narrow band as side', () => {
    expect(classifyDropOutlineAxis(L * 0.5, H, L, H)).toBe('side');
    expect(classifyDropOutlineAxis(L * 0.25, H * 0.9, L, H)).toBe('side');
  });
});
