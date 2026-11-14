import { useEffect, useState, type ComponentType } from 'react';
import { probeTileGenTrace } from './tileGenTrace';
import type { TileGenStressPanelProps } from './TileGenStressPanel';

/**
 * Loads the tile-gen stress panel only after Rust reports DITHER_TRACE.
 * The runners stay out of the product bundle until then.
 */
export default function StressHarnessSlot(props: TileGenStressPanelProps) {
  const [Panel, setPanel] = useState<ComponentType<TileGenStressPanelProps> | null>(null);

  useEffect(() => {
    let cancelled = false;
    let attempts = 0;
    let timer = 0;

    const tryProbe = () => {
      void probeTileGenTrace().then((on) => {
        if (cancelled) return;
        if (on) {
          void import('./TileGenStressPanel').then((mod) => {
            if (!cancelled) setPanel(() => mod.default);
          });
          return;
        }
        attempts += 1;
        if (attempts < 12) timer = window.setTimeout(tryProbe, 500);
      });
    };

    tryProbe();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, []);

  if (!Panel) return null;
  return <Panel {...props} />;
}
