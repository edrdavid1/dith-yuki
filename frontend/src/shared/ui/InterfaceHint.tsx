import type { ReactNode } from 'react';
import { useTooltipsEnabled } from './useTooltipsEnabled';

/**
 * Always-visible explanatory copy (palette banners, field hints).
 * Hidden together with hover tooltips via Settings → Show tooltips.
 */
export default function InterfaceHint({ children }: { children: ReactNode }) {
  const enabled = useTooltipsEnabled();
  if (!enabled) return null;
  return <>{children}</>;
}
