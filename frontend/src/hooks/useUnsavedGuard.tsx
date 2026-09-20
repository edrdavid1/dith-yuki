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
  type UnsavedMultiChoice,
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

type PromptState =
  | { mode: 'single'; basename: string }
  | { mode: 'multi'; documents: UnsavedDocumentRef[]; selectedIds: number[] };

/**
 * Single UnsavedGuard owner for the window:
 * - tab × → one document (+ soft-discard Restore toast)
 * - quit / window close / updater restart → all dirty tabs (aggregated when >1)
 */
export function useUnsavedGuard() {
  const dispatch = useAppDispatch();
  const tabs = useAppSelector((s) => s.tabs.tabs);

  const [prompt, setPrompt] = useState<PromptState | null>(null);
  const singleResolver = useRef<((choice: UnsavedGuardChoice) => void) | null>(null);
  const multiResolver = useRef<((choice: UnsavedMultiChoice) => void) | null>(null);
  const [softDiscard, setSoftDiscard] = useState<SoftDiscardInfo | null>(null);

  const clearPrompt = useCallback(() => {
    setPrompt(null);
    singleResolver.current = null;
    multiResolver.current = null;
  }, []);

  const promptSingle = useCallback(async (doc: UnsavedDocumentRef) => {
    setPrompt({ mode: 'single', basename: displayNameForUnsaved(doc) });
    return new Promise<UnsavedGuardChoice>((resolve) => {
      singleResolver.current = resolve;
    });
  }, []);

  const promptMulti = useCallback(async (docs: UnsavedDocumentRef[]) => {
    setPrompt({
      mode: 'multi',
      documents: docs,
      selectedIds: docs.map((d) => d.id),
    });
    return new Promise<UnsavedMultiChoice>((resolve) => {
      multiResolver.current = resolve;
    });
  }, []);

  const finishSingle = useCallback(
    (choice: UnsavedGuardChoice) => {
      const resolve = singleResolver.current;
      clearPrompt();
      resolve?.(choice);
    },
    [clearPrompt]
  );

  const finishMulti = useCallback(
    (choice: UnsavedMultiChoice) => {
      const resolve = multiResolver.current;
      clearPrompt();
      resolve?.(choice);
    },
    [clearPrompt]
  );

  const toggleSelected = useCallback((id: number) => {
    setPrompt((prev) => {
      if (!prev || prev.mode !== 'multi') return prev;
      const has = prev.selectedIds.includes(id);
      return {
        ...prev,
        selectedIds: has
          ? prev.selectedIds.filter((x) => x !== id)
          : [...prev.selectedIds, id],
      };
    });
  }, []);

  const saveDoc = useCallback(
    (doc: UnsavedDocumentRef) => saveUnsavedDocument(dispatch, doc),
    [dispatch]
  );

  /** Backend snapshot before guard — safety net if live dirty patches were missed. */
  const loadFreshTabs = useCallback(async (): Promise<OpenDocumentTab[]> => {
    const result = await dispatch(refreshTabs());
    if (refreshTabs.fulfilled.match(result)) {
      return result.payload.tabs;
    }
    return tabs;
  }, [dispatch, tabs]);

  /** Quit / close window / restart. */
  const confirmQuit = useCallback(async () => {
    const fresh = await loadFreshTabs();
    return confirmUnsavedDocuments({
      documents: fresh.map(tabToRef),
      promptSingle,
      promptMulti,
      save: saveDoc,
    });
  }, [loadFreshTabs, promptMulti, promptSingle, saveDoc]);

  /** Tab strip × — guard that tab, then close (soft discard keeps journal + toast). */
  const confirmCloseTab = useCallback(
    async (tab: OpenDocumentTab) => {
      const fresh = await loadFreshTabs();
      const current = fresh.find((t) => t.id === tab.id) ?? tab;

      if (!current.dirty) {
        await dispatch(closeTab(current.id));
        return true;
      }

      const choice = await promptSingle(tabToRef(current));
      if (choice === 'cancel') return false;

      if (choice === 'save') {
        const saved = await saveDoc(tabToRef(current));
        if (!saved) return false;
        await dispatch(closeTab(current.id));
        return true;
      }

      // Don’t Save — flush journal, close, offer Restore for ~12s.
      try {
        const info = await prepareSoftDiscard(current.id);
        await dispatch(closeTab(current.id));
        setSoftDiscard(info);
      } catch (err) {
        console.error('Soft discard prepare failed:', err);
        await dispatch(closeTab(current.id));
      }
      return true;
    },
    [dispatch, loadFreshTabs, promptSingle, saveDoc]
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
        isOpen={prompt != null}
        basename={prompt?.mode === 'single' ? prompt.basename : undefined}
        documents={prompt?.mode === 'multi' ? prompt.documents : undefined}
        selectedIds={prompt?.mode === 'multi' ? prompt.selectedIds : undefined}
        onToggleSelected={toggleSelected}
        onSave={() => {
          if (prompt?.mode === 'multi') {
            finishMulti({ kind: 'save-selected', selectedIds: prompt.selectedIds });
          } else {
            finishSingle('save');
          }
        }}
        onDiscard={() => {
          if (prompt?.mode === 'multi') {
            finishMulti({ kind: 'discard-all' });
          } else {
            finishSingle('discard');
          }
        }}
        onCancel={() => {
          if (prompt?.mode === 'multi') {
            finishMulti({ kind: 'cancel' });
          } else {
            finishSingle('cancel');
          }
        }}
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
