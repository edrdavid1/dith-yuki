import { useCallback, useRef, useState } from 'react';
import UnsavedGuardDialog from '../components/UnsavedGuardDialog';
import { useAppDispatch, useAppSelector } from '../app/hooks';
import { closeTab } from '../app/slices/tabsSlice';
import { saveUnsavedDocument } from '../shared/saveDocument';
import {
  confirmUnsavedDocuments,
  confirmUnsavedIfNeeded,
  displayNameForUnsaved,
  type UnsavedDocumentRef,
  type UnsavedGuardChoice,
} from '../shared/unsavedGuard';
import type { OpenDocumentTab } from '../shared/ipc/document';

function tabToRef(tab: OpenDocumentTab): UnsavedDocumentRef {
  return { id: tab.id, dirty: tab.dirty, path: tab.path, title: tab.title };
}

/**
 * Single UnsavedGuard owner for the window (VS Code / Photoshop):
 * - tab × → one document
 * - quit / window close / updater restart → every dirty tab in order
 */
export function useUnsavedGuard() {
  const dispatch = useAppDispatch();
  const tabs = useAppSelector((s) => s.tabs.tabs);

  const [open, setOpen] = useState(false);
  const [basename, setBasename] = useState('Untitled');
  const resolver = useRef<((choice: UnsavedGuardChoice) => void) | null>(null);

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

  /** Tab strip × — guard that tab, then close. */
  const confirmCloseTab = useCallback(
    async (tab: OpenDocumentTab) => {
      const ok = await confirmUnsavedIfNeeded({
        hasDocument: true,
        dirty: tab.dirty,
        prompt: () => promptFor(tabToRef(tab)),
        save: () => saveDoc(tabToRef(tab)),
      });
      if (ok) {
        await dispatch(closeTab(tab.id));
      }
      return ok;
    },
    [dispatch, promptFor, saveDoc]
  );

  const dialog = (
    <UnsavedGuardDialog
      isOpen={open}
      basename={basename}
      onSave={() => finish('save')}
      onDiscard={() => finish('discard')}
      onCancel={() => finish('cancel')}
    />
  );

  return {
    confirmQuit,
    confirmCloseTab,
    dialog,
  };
}
