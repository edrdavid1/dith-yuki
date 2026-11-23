/**
 * Frontend mirror of document `GamedevContext` (engine is source of truth).
 * Release slice: Tile + Sprites + Output (Animation / frame-set deferred).
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useSelector } from 'react-redux';
import { listen } from '@tauri-apps/api/event';
import type { RootState } from '../../app/store';
import {
  defaultGamedevContext,
  gamedevChipAction,
  getGamedevChips,
  getGamedevContext,
  refreshGamedevChips,
  setGamedevContext,
  type CellSettings,
  type GamedevCacheLayer,
  type GamedevChip,
  type GamedevContext as GamedevContextDto,
} from '../../shared/ipc/gamedev';
import { onFullDocumentBusy } from '../../shared/ipc/events';
import { useWorkspaceOptional } from './WorkspaceContext';

type GamedevStoreValue = {
  ready: boolean;
  docId: number | null;
  context: GamedevContextDto;
  chips: GamedevChip[];
  lastInvalidated: GamedevCacheLayer[];
  /** Bind mirror to the active document; loads engine state. */
  attachDocument: (docId: number | null) => Promise<void>;
  /** Replace context via IPC. */
  updateContext: (next: GamedevContextDto) => Promise<GamedevCacheLayer[]>;
  setPipelineTile: (enabled: boolean) => Promise<void>;
  setCellSettings: (cell: CellSettings | null) => Promise<void>;
  setNearestPreview: (on: boolean) => Promise<void>;
  setIntegerUpscale: (factor: number | null) => Promise<void>;
  setExportPng8: (on: boolean) => Promise<void>;
  /** F5: apply Gamedev defaults (PNG8, nearest preview) without clobbering custom context. */
  applyGamedevDefaults: () => Promise<void>;
  refreshChips: () => Promise<void>;
  runChipAction: (actionId: string) => Promise<void>;
};

const GamedevStoreContext = createContext<GamedevStoreValue | null>(null);

/** Release: clear deferred Animation job / frame_set so exclusive Tile↔Sprites stay clean. */
function withoutDeferredAnimation(ctx: GamedevContextDto): GamedevContextDto {
  const hasAnim =
    ctx.job === 'animation' || (ctx.frame_set != null && ctx.frame_set.length > 0);
  if (!hasAnim) return ctx;
  return {
    ...ctx,
    job: ctx.job === 'animation' ? 'none' : ctx.job,
    frame_set: [],
  };
}

function GamedevDocumentBinder({
  attachDocument,
}: {
  attachDocument: (docId: number | null) => Promise<void>;
}) {
  const docId = useSelector((s: RootState) => s.document.docId);
  useEffect(() => {
    void attachDocument(docId);
  }, [attachDocument, docId]);
  return null;
}

/** F5: when entering Gamedev workspace, bias PNG8 + nearest preview + hard alpha. */
function GamedevDefaultsOnEnter({
  applyGamedevDefaults,
}: {
  applyGamedevDefaults: () => Promise<void>;
}) {
  const workspace = useWorkspaceOptional();
  const prevRef = useRef(workspace?.workspace);
  useEffect(() => {
    const next = workspace?.workspace;
    if (next === 'gamedev' && prevRef.current !== 'gamedev') {
      void applyGamedevDefaults();
    }
    prevRef.current = next;
  }, [workspace?.workspace, applyGamedevDefaults]);
  return null;
}

export function GamedevProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [docId, setDocId] = useState<number | null>(null);
  const [context, setContext] = useState<GamedevContextDto>(defaultGamedevContext);
  const [chips, setChips] = useState<GamedevChip[]>([]);
  const [lastInvalidated, setLastInvalidated] = useState<GamedevCacheLayer[]>([]);

  const attachDocument = useCallback(async (id: number | null) => {
    setDocId(id);
    if (id == null) {
      setContext(defaultGamedevContext());
      setChips([]);
      setLastInvalidated([]);
      setReady(true);
      return;
    }
    try {
      let ctx = await getGamedevContext(id);
      const cleaned = withoutDeferredAnimation(ctx);
      if (cleaned !== ctx) {
        const res = await setGamedevContext(id, cleaned);
        ctx = res.context;
        setLastInvalidated(res.invalidated);
      }
      setContext(ctx);
      const nextChips = await getGamedevChips(id);
      setChips(nextChips);
    } catch (err) {
      console.error('gamedev attach failed', err);
      setContext(defaultGamedevContext());
      setChips([]);
    } finally {
      setReady(true);
    }
  }, []);

  const updateContext = useCallback(
    async (next: GamedevContextDto) => {
      if (docId == null) return [];
      const res = await setGamedevContext(docId, withoutDeferredAnimation(next));
      setContext(res.context);
      setLastInvalidated(res.invalidated);
      return res.invalidated;
    },
    [docId],
  );

  const setPipelineTile = useCallback(
    async (enabled: boolean) => {
      if (docId == null) return;
      // Exclusive: Tile clears Sprites; animation always cleared in release slice.
      await updateContext({
        ...context,
        tile: { enabled },
        job: enabled ? 'tile' : context.job === 'tile' ? 'none' : context.job,
        cell: enabled ? null : context.cell,
        frame_set: [],
      });
    },
    [context, docId, updateContext],
  );

  const setCellSettings = useCallback(
    async (cell: CellSettings | null) => {
      if (docId == null) return;
      // Exclusive vs Tile; animation always cleared in release slice.
      await updateContext({
        ...context,
        cell,
        tile: cell != null ? { enabled: false } : context.tile,
        job: cell != null ? 'sprite' : context.job === 'sprite' ? 'none' : context.job,
        frame_set: [],
      });
    },
    [context, docId, updateContext],
  );

  const setNearestPreview = useCallback(
    async (on: boolean) => {
      if (docId == null) return;
      await updateContext({
        ...context,
        pixel_perfect: { ...context.pixel_perfect, nearest_preview: on },
      });
    },
    [context, docId, updateContext],
  );

  const setIntegerUpscale = useCallback(
    async (factor: number | null) => {
      if (docId == null) return;
      const integer_upscale =
        factor == null || factor <= 1 ? null : Math.min(16, Math.max(2, Math.round(factor)));
      await updateContext({
        ...context,
        pixel_perfect: { ...context.pixel_perfect, integer_upscale },
      });
    },
    [context, docId, updateContext],
  );

  const setExportPng8 = useCallback(
    async (on: boolean) => {
      if (docId == null) return;
      await updateContext({
        ...context,
        export: { ...context.export, png8: on },
      });
    },
    [context, docId, updateContext],
  );

  const applyGamedevDefaults = useCallback(async () => {
    if (docId == null) return;
    await updateContext({
      ...context,
      export: { ...context.export, png8: true },
      pixel_perfect: { ...context.pixel_perfect, nearest_preview: true },
      alpha: {
        ...context.alpha,
        no_dither_alpha: true,
        clear_rgb_in_transparent: true,
      },
    });
  }, [context, docId, updateContext]);

  const refreshChips = useCallback(async () => {
    if (docId == null) return;
    await refreshGamedevChips(docId);
  }, [docId]);

  const runChipAction = useCallback(
    async (actionId: string) => {
      if (docId == null) return;
      const res = await gamedevChipAction(docId, actionId);
      setContext(withoutDeferredAnimation(res.context));
    },
    [docId],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<{ docId: number; chips: GamedevChip[] }>('gamedev-chips', (ev) => {
      if (docId != null && ev.payload.docId === docId) {
        setChips(ev.payload.chips);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [docId]);

  // Revalidate heuristics when non-gamedev edits land. Gamedev set/chip-action
  // already spawn validation in Rust — refreshing here only stacked duplicate work.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<{ doc_id?: number | null; kind?: string }>('document-changed', (ev) => {
      const id = ev.payload.doc_id;
      if (docId == null || id !== docId) return;
      const kind = ev.payload.kind ?? '';
      if (kind.startsWith('gamedev_')) return;
      void refreshGamedevChips(docId).catch(() => {});
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [docId]);

  // After preview FD settle, validate seams against the cached result (no recompute).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onFullDocumentBusy((ev) => {
      if (docId == null || ev.payload.doc_id !== docId) return;
      if (ev.payload.busy) return;
      void refreshGamedevChips(docId).catch(() => {});
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [docId]);

  const value = useMemo<GamedevStoreValue>(
    () => ({
      ready,
      docId,
      context,
      chips,
      lastInvalidated,
      attachDocument,
      updateContext,
      setPipelineTile,
      setCellSettings,
      setNearestPreview,
      setIntegerUpscale,
      setExportPng8,
      applyGamedevDefaults,
      refreshChips,
      runChipAction,
    }),
    [
      ready,
      docId,
      context,
      chips,
      lastInvalidated,
      attachDocument,
      updateContext,
      setPipelineTile,
      setCellSettings,
      setNearestPreview,
      setIntegerUpscale,
      setExportPng8,
      applyGamedevDefaults,
      refreshChips,
      runChipAction,
    ],
  );

  return (
    <GamedevStoreContext.Provider value={value}>
      <GamedevDocumentBinder attachDocument={attachDocument} />
      <GamedevDefaultsOnEnter applyGamedevDefaults={applyGamedevDefaults} />
      {children}
    </GamedevStoreContext.Provider>
  );
}

export function useGamedevStore(): GamedevStoreValue {
  const ctx = useContext(GamedevStoreContext);
  if (!ctx) {
    throw new Error('useGamedevStore must be used within GamedevProvider');
  }
  return ctx;
}

export function useGamedevStoreOptional(): GamedevStoreValue | null {
  return useContext(GamedevStoreContext);
}
