import { useCallback, useEffect, useState } from 'react';
import { getRecentFiles, type RecentFileEntry } from '../shared/ipc/recent';

export function useRecentFiles() {
  const [entries, setEntries] = useState<RecentFileEntry[]>([]);
  const refresh = useCallback(async () => {
    try {
      setEntries(await getRecentFiles());
    } catch {
      setEntries([]);
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return { entries, refresh };
}
