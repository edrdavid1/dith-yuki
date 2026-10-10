/**
 * Workspace runtime state (FR-3…FR-9). Visibility is the only gate for UI.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  evaluateWorkspaceHint,
  getAppConfig,
  noteTileExport,
  noteWorkspacePromptShown,
  resolveWorkspaceHint,
  resolveWorkspacePrompt,
  setTilingAdvanced,
  setWorkspace,
  type AppConfigDto,
} from '../../shared/ipc/workspace';
import {
  uiVisibilityFromConfig,
  type UiVisibility,
  type Workspace,
} from '../../shared/workspace/visibility';

const DEFAULT_VISIBILITY: UiVisibility = uiVisibilityFromConfig({
  workspace: 'general',
  advanced: { tiling_advanced: false },
});

type WorkspaceContextValue = {
  ready: boolean;
  workspace: Workspace;
  visibility: UiVisibility;
  tilingAdvanced: boolean;
  showWorkspacePrompt: boolean;
  hintVisible: boolean;
  setWorkspacePreference: (w: Workspace) => Promise<void>;
  setTilingAdvancedPreference: (enabled: boolean) => Promise<void>;
  resolvePrompt: (choice: Workspace, remember: boolean) => Promise<void>;
  triggerHint: (args: {
    trigger: string;
    filename?: string;
    docWidth?: number;
    docHeight?: number;
    edEnabled?: boolean;
  }) => Promise<void>;
  dismissHint: (action?: 'dismiss' | 'timeout') => Promise<void>;
  switchFromHint: () => Promise<void>;
  markTileExport: () => Promise<void>;
};

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

function applyDto(dto: AppConfigDto): {
  workspace: Workspace;
  visibility: UiVisibility;
  tilingAdvanced: boolean;
  showWorkspacePrompt: boolean;
} {
  return {
    workspace: dto.workspace,
    visibility: dto.visibility ?? uiVisibilityFromConfig({
      workspace: dto.workspace,
      advanced: dto.advanced,
    }),
    tilingAdvanced: Boolean(dto.advanced?.tiling_advanced),
    showWorkspacePrompt: Boolean(dto.showWorkspacePrompt),
  };
}

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [workspace, setWorkspaceState] = useState<Workspace>('general');
  const [visibility, setVisibility] = useState<UiVisibility>(DEFAULT_VISIBILITY);
  const [tilingAdvanced, setTilingAdvancedState] = useState(false);
  const [showWorkspacePrompt, setShowPrompt] = useState(false);
  const [hintVisible, setHintVisible] = useState(false);
  const [hintShownThisSession, setHintShownThisSession] = useState(false);

  const ingest = useCallback((dto: AppConfigDto) => {
    const next = applyDto(dto);
    setWorkspaceState(next.workspace);
    setVisibility(next.visibility);
    setTilingAdvancedState(next.tilingAdvanced);
    setShowPrompt(next.showWorkspacePrompt);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getAppConfig()
      .then(async (dto) => {
        if (cancelled) return;
        ingest(dto);
        if (dto.showWorkspacePrompt) {
          try {
            await noteWorkspacePromptShown();
          } catch {
            /* ignore */
          }
        }
        setReady(true);
      })
      .catch(() => {
        if (!cancelled) {
          setReady(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [ingest]);

  const setWorkspacePreference = useCallback(
    async (w: Workspace) => {
      const dto = await setWorkspace(w, 'settings');
      ingest(dto);
      if (w === 'gamedev') setHintVisible(false);
    },
    [ingest],
  );

  const setTilingAdvancedPreference = useCallback(
    async (enabled: boolean) => {
      const dto = await setTilingAdvanced(enabled);
      ingest(dto);
    },
    [ingest],
  );

  const resolvePrompt = useCallback(
    async (choice: Workspace, remember: boolean) => {
      const dto = await resolveWorkspacePrompt(choice, remember);
      ingest(dto);
      setShowPrompt(false);
    },
    [ingest],
  );

  const triggerHint = useCallback(
    async (args: {
      trigger: string;
      filename?: string;
      docWidth?: number;
      docHeight?: number;
      edEnabled?: boolean;
    }) => {
      if (hintShownThisSession || workspace !== 'general') return;
      try {
        const decision = await evaluateWorkspaceHint(args);
        if (decision.show) {
          setHintVisible(true);
          setHintShownThisSession(true);
        }
      } catch {
        /* ignore offline / test */
      }
    },
    [hintShownThisSession, workspace],
  );

  const dismissHint = useCallback(async (action: 'dismiss' | 'timeout' = 'dismiss') => {
    setHintVisible(false);
    try {
      await resolveWorkspaceHint(action);
    } catch {
      /* ignore */
    }
  }, []);

  const switchFromHint = useCallback(async () => {
    setHintVisible(false);
    try {
      const dto = await resolveWorkspaceHint('switch');
      ingest(dto);
    } catch {
      /* ignore */
    }
  }, [ingest]);

  const markTileExport = useCallback(async () => {
    try {
      await noteTileExport();
    } catch {
      /* ignore */
    }
  }, []);

  const value = useMemo(
    () => ({
      ready,
      workspace,
      visibility,
      tilingAdvanced,
      showWorkspacePrompt,
      hintVisible,
      setWorkspacePreference,
      setTilingAdvancedPreference,
      resolvePrompt,
      triggerHint,
      dismissHint,
      switchFromHint,
      markTileExport,
    }),
    [
      ready,
      workspace,
      visibility,
      tilingAdvanced,
      showWorkspacePrompt,
      hintVisible,
      setWorkspacePreference,
      setTilingAdvancedPreference,
      resolvePrompt,
      triggerHint,
      dismissHint,
      switchFromHint,
      markTileExport,
    ],
  );

  return (
    <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>
  );
}

export function useWorkspace(): WorkspaceContextValue {
  const ctx = useContext(WorkspaceContext);
  if (!ctx) {
    throw new Error('useWorkspace must be used within WorkspaceProvider');
  }
  return ctx;
}

/** Safe hook for optional trees (defaults to General / all hidden). */
export function useWorkspaceOptional(): WorkspaceContextValue | null {
  return useContext(WorkspaceContext);
}
