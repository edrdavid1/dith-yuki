import { useCallback, useRef, useState } from 'react';
import UnsavedGuardDialog from '../components/UnsavedGuardDialog';
import DiscardRestoreToast from '../components/DiscardRestoreToast';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { closeTab, refreshTabs } from '../app/slices/tabsSlice';
import { refreshDocument } from '../app/slices/documentSlice';
import { saveUnsavedDocument } from '../shared/saveDocument';
import {
  confirmUnsavedDocuments,
  displayNameForUnsaved,
  type UnsavedDocumentRef,
  type UnsavedGuardChoice,
} from '../shared/unsavedGuard';
import type { OpenDocumentTab } from '../shared/ipc/document';
import {
  prepareSoftDiscard,
  recoverJournal,
  type SoftDiscardInfo,
} from '../shared/ipc/recovery';

function tabToRef(tab: OpenDocumentTab): UnsavedDocumentRef {
  return { id: tab.id, dirty: tab.dirty, path: tab.path, title: tab.title };
}

/**
 * Single UnsavedGuard owner for the window (VS Code / Photoshop):
 * - tab × → one document (+ soft-discard Restore toast)
 * - quit / window close / updater restart → every dirty tab in order
 */
export function useUnsavedGuard() {
  const dispatch = useAppDispatch();
  const tabs = useAppSelector((s) => s.tabs.tabs);

  const [open, setOpen] = useState(false);
  const [basename, setBasename] = useState('Untitled');
  const resolver = useRef<((choice: UnsavedGuardChoice) => void) | null>(null);
  const [softDiscard, setSoftDiscard] = useState<SoftDiscardInfo | null>(null);

  const promptFor = useCallback(async (doc: UnsavedDocumentRef) => {
    setBasename(displayNameForUnsaved(doc));
    return new Promise<UnsavedGuardChoice>((resolve) => {
      resolver.current = resolve;
      setOpen(true);
    });
  }, []);

  const finish = useCallback((choice: UnsavedGuardChoice) => {
    setOpen(false);
    resolver.current?.(choice);
    resolver.current = null;
  }, []);

  const saveDoc = useCallback(
    (doc: UnsavedDocumentRef) => saveUnsavedDocument(dispatch, doc),
    [dispatch]
  );

  /** Quit / close window / restart: walk all dirty tabs sequentially. */
  const confirmQuit = useCallback(async () => {
    return confirmUnsavedDocuments({
      documents: tabs.map(tabToRef),
      promptFor,
      save: saveDoc,
    });
  }, [promptFor, saveDoc, tabs]);

  /** Tab strip × — guard that tab, then close (soft discard keeps journal + toast). */
  const confirmCloseTab = useCallback(
    async (tab: OpenDocumentTab) => {
      if (!tab.dirty) {
        await dispatch(closeTab(tab.id));
        return true;
      }

      const choice = await promptFor(tabToRef(tab));
      if (choice === 'cancel') return false;

      if (choice === 'save') {
        const saved = await saveDoc(tabToRef(tab));
        if (!saved) return false;
        await dispatch(closeTab(tab.id));
        return true;
      }

      // Don’t Save — flush journal, close, offer Restore for ~12s.
      try {
        const info = await prepareSoftDiscard(tab.id);
        await dispatch(closeTab(tab.id));
        setSoftDiscard(info);
      } catch (err) {
        console.error('Soft discard prepare failed:', err);
        await dispatch(closeTab(tab.id));
      }
      return true;
    },
    [dispatch, promptFor, saveDoc]
  );

  const onRestoreDiscarded = useCallback(
    async (info: SoftDiscardInfo) => {
      setSoftDiscard(null);
      try {
        await recoverJournal(info.recovery_id);
        await dispatch(refreshTabs());
        await dispatch(refreshDocument());
      } catch (err) {
        console.error('Restore discarded tab failed:', err);
      }
    },
    [dispatch]
  );

  const dialog = (
    <>
      <UnsavedGuardDialog
        isOpen={open}
        basename={basename}
        onSave={() => finish('save')}
        onDiscard={() => finish('discard')}
        onCancel={() => finish('cancel')}
      />
      <DiscardRestoreToast
        info={softDiscard}
        onRestore={(info) => void onRestoreDiscarded(info)}
        onDismiss={() => setSoftDiscard(null)}
      />
    </>
  );

  return {
    confirmQuit,
    confirmCloseTab,
    dialog,
  };
}
