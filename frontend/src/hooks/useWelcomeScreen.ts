import { useCallback, useState } from 'react';
import { useDocument } from './useDocument';
import { useRecentFiles } from './useRecentFiles';
import { openRecentByKind, type RecentFileEntry } from '../shared/ipc/recent';
import type { BlankBackground } from '../shared/ipc/document';

export interface WelcomeActions {
  recentEntries: RecentFileEntry[];
  onNewProject: () => void;
  onOpenImage: () => void;
  onOpenProject: () => void;
  onOpenRecent: (entry: RecentFileEntry) => void;
}

/**
 * One Recent source + shared open/create/save wrappers per window.
 * AppLayout (main) and floating Preview both use this; they are separate JS trees.
 */
export function useWelcomeScreen() {
  const doc = useDocument();
  const { entries, refresh } = useRecentFiles();
  const [newProjectOpen, setNewProjectOpen] = useState(false);

  const runAndRefresh = useCallback(
    async (op: () => Promise<void>) => {
      await op();
      await refresh();
    },
    [refresh]
  );

  const onNewProject = useCallback(() => {
    setNewProjectOpen(true);
  }, []);

  const onOpenImage = useCallback(() => {
    void runAndRefresh(doc.openImage);
  }, [doc.openImage, runAndRefresh]);

  const onOpenProject = useCallback(() => {
    void runAndRefresh(doc.openProject);
  }, [doc.openProject, runAndRefresh]);

  const onOpenRecent = useCallback(
    (entry: RecentFileEntry) => {
      void runAndRefresh(async () => {
        await openRecentByKind(entry, {
          openImageAt: doc.openImageAt,
          openProjectAt: doc.openProjectAt,
        });
      });
    },
    [doc.openImageAt, doc.openProjectAt, runAndRefresh]
  );

  const onSaveImage = useCallback(() => {
    void runAndRefresh(doc.saveImage);
  }, [doc.saveImage, runAndRefresh]);

  const onSaveProject = useCallback(() => {
    void runAndRefresh(doc.saveProject);
  }, [doc.saveProject, runAndRefresh]);

  const onSaveProjectAs = useCallback(() => {
    void runAndRefresh(doc.saveProjectAs);
  }, [doc.saveProjectAs, runAndRefresh]);

  const handleCreate = useCallback(
    async (args: { width: number; height: number; background: BlankBackground }) => {
      const ok = await doc.createDocument(args);
      if (ok) {
        setNewProjectOpen(false);
        await refresh();
      }
    },
    [doc.createDocument, refresh]
  );

  const welcome: WelcomeActions = {
    recentEntries: entries,
    onNewProject,
    onOpenImage,
    onOpenProject,
    onOpenRecent,
  };

  return {
    doc,
    welcome,
    newProjectOpen,
    closeNewProject: () => setNewProjectOpen(false),
    handleCreate,
    onSaveImage,
    onSaveProject,
    onSaveProjectAs,
  };
}
