import { useCallback, useEffect, useState } from 'react';
import AppLayout from './app/AppLayout';
import RecoveryDialog from './components/RecoveryDialog';
import {
  discardRecoveryJournals,
  recoverJournal,
  scanRecoveryJournals,
  type JournalMeta,
} from './shared/ipc/recovery';
import { useAppDispatch } from './app/hooks';
import { refreshTabs } from './app/slices/tabsSlice';
import { refreshDocument } from './app/slices/documentSlice';

/** Thin app root — providers live in main.tsx; layout owns the shell. */
function App() {
  const dispatch = useAppDispatch();
  const [journals, setJournals] = useState<JournalMeta[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void scanRecoveryJournals()
      .then((scan) => {
        if (cancelled) return;
        // Show only when journals exist (first launch also lacks marker).
        if (scan.journals.length > 0) {
          setJournals(scan.journals);
        } else {
          setJournals([]);
        }
      })
      .catch((err) => {
        console.error('Recovery scan failed:', err);
        if (!cancelled) setJournals([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const finish = useCallback(async () => {
    setJournals([]);
    await dispatch(refreshTabs());
    await dispatch(refreshDocument());
  }, [dispatch]);

  const onRecoverAll = useCallback(async () => {
    if (!journals) return;
    setBusy(true);
    try {
      for (const j of journals) {
        await recoverJournal(j.recovery_id);
      }
      await finish();
    } catch (err) {
      console.error('Recover failed:', err);
    } finally {
      setBusy(false);
    }
  }, [finish, journals]);

  const onDiscardAll = useCallback(async () => {
    setBusy(true);
    try {
      await discardRecoveryJournals();
      await finish();
    } catch (err) {
      console.error('Discard journals failed:', err);
    } finally {
      setBusy(false);
    }
  }, [finish]);

  const onSkip = useCallback(async () => {
    // Keep journals on disk for a later session; just continue.
    setJournals([]);
  }, []);

  const scanning = journals === null;
  const showRecovery = (journals?.length ?? 0) > 0;

  return (
    <>
      {!scanning && !showRecovery ? <AppLayout /> : null}
      {showRecovery && journals ? (
        <RecoveryDialog
          journals={journals}
          busy={busy}
          onRecoverAll={() => void onRecoverAll()}
          onDiscardAll={() => void onDiscardAll()}
          onSkip={() => void onSkip()}
        />
      ) : null}
    </>
  );
}

export default App;
