/**
 * Prefer the less-crowded sidebar when injecting Gamedev Settings
 * (typically the side that already has a single panel, e.g. Layers).
 */

import type { Model } from 'flexlayout-react';
import { listDockedFlexComponents } from '../../contexts/layoutModelOps';

export function preferredSideForGamedevPanel(
  left: Model | null,
  right: Model | null,
): 'left' | 'right' {
  const leftN = listDockedFlexComponents(left).length;
  const rightN = listDockedFlexComponents(right).length;
  if (leftN < rightN) return 'left';
  if (rightN < leftN) return 'right';
  return 'left';
}
