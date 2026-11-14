import { useCallback, useEffect, useState } from 'react';
import {
  getEntitlement,
  refreshLicense,
  type Entitlement,
} from './index';

export function useEntitlement(enabled: boolean) {
  const [entitlement, setEntitlement] = useState<Entitlement | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;
    try {
      const next = await refreshLicense();
      setEntitlement(next);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      setEntitlement({ state: 'unlocked' });
      return;
    }
    let cancelled = false;
    void getEntitlement()
      .then((e) => {
        if (!cancelled) setEntitlement(e);
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);

  return { entitlement, error, refresh };
}
