import { useCallback, useEffect, useState } from 'react';
import { clearRecentFiles, getRecentFiles, type RecentFileEntry } from '../shared/ipc/recent';

export function useRecentFiles() {
  const [entries, setEntries] = useState<RecentFileEntry[]>([]);
  const refresh = useCallback(async () => {
    try {
      setEntries(await getRecentFiles());
    } catch {
      setEntries([]);
    }
  }, []);
  const clear = useCallback(async () => {
    try {
      await clearRecentFiles();
      setEntries([]);
    } catch {
      await refresh();
    }
  }, [refresh]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return { entries, refresh, clear };
}
