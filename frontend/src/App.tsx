import { useCallback, useEffect, useRef, useState } from 'react';
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
import { useAppDispatch, useAppSelector } from './app/hooks';
import { refreshTabs } from './app/slices/tabsSlice';
import { refreshDocument } from './app/slices/documentSlice';
import { finishBoot } from './lib/boot';
import { useLayoutContext } from './contexts/LayoutContext';
import { welcomeBackgroundStyle } from './features/preview/welcomeBackground';

type RecoveryGate =
  | { kind: 'scanning' }
  | { kind: 'none' }
  | { kind: 'journals'; journals: JournalMeta[]; rosterDocs: RosterEntry[] }
  | { kind: 'roster'; rosterDocs: RosterEntry[] };

/** Thin app root — providers live in main.tsx; layout owns the shell. */
function App() {
  const dispatch = useAppDispatch();
  const { center } = useLayoutContext();
  const documentHydrated = useAppSelector((s) => s.document.hydrated);
  const [gate, setGate] = useState<RecoveryGate>({ kind: 'scanning' });
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

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

  // Reveal window (and fade splash only if the slow path actually showed it).
  useEffect(() => {
    if (gate.kind === 'scanning') return;
    // Welcome/shell needs FlexLayout + document hydrate; recovery dialog does not.
    if (gate.kind === 'none' && (center.isLoading || !documentHydrated)) return;

    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        finishBoot();
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      if (raf2) cancelAnimationFrame(raf2);
    };
  }, [gate.kind, center.isLoading, documentHydrated]);

  const finish = useCallback(async () => {
    setGate({ kind: 'none' });
    await dispatch(refreshTabs());
    await dispatch(refreshDocument());
  }, [dispatch]);

  const onRecoverAll = useCallback(async () => {
    if (gate.kind !== 'journals') return;
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      // One tab per recovery_id (guards double-click / overlapping invokes).
      const seen = new Set<string>();
      for (const j of gate.journals) {
        if (seen.has(j.recovery_id)) continue;
        seen.add(j.recovery_id);
        try {
          await recoverJournal(j.recovery_id);
        } catch (err) {
          // Already claimed by a concurrent recover — skip.
          console.error(`Recover failed for ${j.recovery_id}:`, err);
        }
      }
      await finish();
    } catch (err) {
      console.error('Recover failed:', err);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [finish, gate]);

  const onDiscardAll = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await discardRecoveryJournals();
      await finish();
    } catch (err) {
      console.error('Discard journals failed:', err);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [finish]);

  const onReopenRoster = useCallback(async () => {
    if (gate.kind !== 'roster') return;
    if (busyRef.current) return;
    busyRef.current = true;
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
      busyRef.current = false;
      setBusy(false);
    }
  }, [finish, gate]);

  const onOpenOriginalsOnly = useCallback(async () => {
    if (gate.kind !== 'journals') {
      setGate({ kind: 'none' });
      return;
    }
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      // Skip journal blobs; reopen the last-session paths when we have them.
      const seen = new Set<string>();
      for (const entry of gate.rosterDocs) {
        const path = entry.project_path ?? entry.source_path;
        if (!path || seen.has(path)) continue;
        seen.add(path);
        try {
          await openProject(path);
        } catch (err) {
          console.error(`Reopen original failed for ${path}:`, err);
        }
      }
      await finish();
    } finally {
      busyRef.current = false;
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
        <>
          {/* Same artwork as boot so crossfade never lands on empty black. */}
          <div
            aria-hidden
            style={{
              position: 'fixed',
              inset: 0,
              zIndex: 499,
              ...welcomeBackgroundStyle('artwork'),
            }}
          />
          <RecoveryDialog
            journals={gate.kind === 'journals' ? gate.journals : []}
            rosterDocs={
              gate.kind === 'journals' || gate.kind === 'roster' ? gate.rosterDocs : []
            }
            busy={busy}
            onRecoverAll={() => void onRecoverAll()}
            onDiscardAll={() => void onDiscardAll()}
            onReopenRoster={gate.kind === 'roster' ? () => void onReopenRoster() : undefined}
            onOpenOriginalsOnly={
              gate.kind === 'journals' ? () => void onOpenOriginalsOnly() : undefined
            }
            onSkip={onSkip}
          />
        </>
      ) : null}
    </>
  );
}

export default App;
