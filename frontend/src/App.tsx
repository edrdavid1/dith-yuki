import { useCallback, useEffect, useState } from 'react';
import AppLayout from './app/AppLayout';
import RecoveryDialog from './components/RecoveryDialog';
import {
  discardRecoveryJournals,
  recoverJournal,
  scanRecoveryJournals,
  type JournalMeta,
  type RosterEntry,
} from './shared/ipc/recovery';
import { openProject } from './shared/ipc/project';
import { useAppDispatch } from './app/hooks';
import { refreshTabs } from './app/slices/tabsSlice';
import { refreshDocument } from './app/slices/documentSlice';

type RecoveryGate =
  | { kind: 'scanning' }
  | { kind: 'none' }
  | { kind: 'journals'; journals: JournalMeta[]; rosterDocs: RosterEntry[] }
  | { kind: 'roster'; rosterDocs: RosterEntry[] };

/** Thin app root — providers live in main.tsx; layout owns the shell. */
function App() {
  const dispatch = useAppDispatch();
  const [gate, setGate] = useState<RecoveryGate>({ kind: 'scanning' });
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void scanRecoveryJournals()
      .then((scan) => {
        if (cancelled) return;
        const rosterDocs = scan.roster?.open_docs ?? [];
        if (scan.journals.length > 0) {
          setGate({ kind: 'journals', journals: scan.journals, rosterDocs });
        } else if (scan.previous_unclean && rosterDocs.length > 0) {
          setGate({ kind: 'roster', rosterDocs });
        } else {
          setGate({ kind: 'none' });
        }
      })
      .catch((err) => {
        console.error('Recovery scan failed:', err);
        if (!cancelled) setGate({ kind: 'none' });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const finish = useCallback(async () => {
    setGate({ kind: 'none' });
    await dispatch(refreshTabs());
    await dispatch(refreshDocument());
  }, [dispatch]);

  const onRecoverAll = useCallback(async () => {
    if (gate.kind !== 'journals') return;
    setBusy(true);
    try {
      for (const j of gate.journals) {
        await recoverJournal(j.recovery_id);
      }
      await finish();
    } catch (err) {
      console.error('Recover failed:', err);
    } finally {
      setBusy(false);
    }
  }, [finish, gate]);

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

  const onReopenRoster = useCallback(async () => {
    if (gate.kind !== 'roster') return;
    setBusy(true);
    try {
      for (const entry of gate.rosterDocs) {
        const path = entry.project_path ?? entry.source_path;
        if (!path) continue;
        try {
          await openProject(path);
        } catch (err) {
          console.error(`Reopen failed for ${path}:`, err);
        }
      }
      await finish();
    } finally {
      setBusy(false);
    }
  }, [finish, gate]);

  const onSkip = useCallback(() => {
    // Keep journals on disk for a later session; just continue.
    setGate({ kind: 'none' });
  }, []);

  const showRecovery = gate.kind === 'journals' || gate.kind === 'roster';

  return (
    <>
      {gate.kind === 'none' ? <AppLayout /> : null}
      {showRecovery ? (
        <RecoveryDialog
          journals={gate.kind === 'journals' ? gate.journals : []}
          rosterDocs={
            gate.kind === 'journals' || gate.kind === 'roster' ? gate.rosterDocs : []
          }
          busy={busy}
          onRecoverAll={() => void onRecoverAll()}
          onDiscardAll={() => void onDiscardAll()}
          onReopenRoster={gate.kind === 'roster' ? () => void onReopenRoster() : undefined}
          onSkip={onSkip}
        />
      ) : null}
    </>
  );
}

export default App;
