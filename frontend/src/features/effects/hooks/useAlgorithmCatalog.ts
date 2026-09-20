import { useEffect, useState } from 'react';
import {
  EFFECT_CATEGORIES,
  listAlgorithmsForCategory,
  type AlgorithmInfo,
} from '../../../shared/ipc/registry';

/** All registered algorithms, grouped by backend category (Req 6.2). */
export function useAlgorithmCatalog(): AlgorithmInfo[] | null {
  const [list, setList] = useState<AlgorithmInfo[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    void Promise.all(EFFECT_CATEGORIES.map((c) => listAlgorithmsForCategory(c)))
      .then((groups) => {
        if (cancelled) return;
        const merged = groups.flat();
        merged.sort((a, b) => {
          if (a.category !== b.category) {
            return a.category.localeCompare(b.category);
          }
          return a.display_name.localeCompare(b.display_name);
        });
        setList(merged);
      })
      .catch(() => {
        if (!cancelled) setList([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return list;
}
