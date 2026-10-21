/**
 * Per-tile generation trace (lost-tiles spec).
 * Off until `tile_gen_trace_enabled` returns true (`DITHER_TRACE=1` on Rust).
 */

import { invoke } from '@tauri-apps/api/core';

let enabled = false;
let probed = false;
let activeRunId: number | null = null;
let ipcCallSeq = 0;

function epochMs(): number {
  return performance.timeOrigin + performance.now();
}

export async function probeTileGenTrace(): Promise<boolean> {
  // Cache only a successful enable; retry after early IPC failures so the
  // Stress panel can appear once the backend is ready.
  if (probed && enabled) return true;
  try {
    enabled = await invoke<boolean>('tile_gen_trace_enabled');
    probed = true;
  } catch {
    enabled = false;
    probed = false;
  }
  return enabled;
}

export function tileGenTraceEnabled(): boolean {
  return enabled;
}

export function activeTraceRunId(): number | null {
  return activeRunId;
}

export async function beginTraceRun(
  scenario?: string,
  seed?: number,
): Promise<number | null> {
  if (!enabled) return null;
  try {
    const id = await invoke<number>('tile_gen_trace_begin_run', {
      req: { scenario, seed },
    });
    activeRunId = id;
    return id;
  } catch {
    return null;
  }
}

type FeDetails = Record<string, unknown>;

export type TraceFeEventOpts = {
  runId?: number | null;
  layerId?: number;
  docGen?: number;
  layerGen?: number;
  level?: number;
  x?: number;
  y?: number;
  gen?: number;
  details?: FeDetails;
};

type TraceFeListener = (event: string, opts?: TraceFeEventOpts) => void;

const feListeners = new Set<TraceFeListener>();

/** Live FE event fan-out (viewport stress ledger). Returns unsubscribe. */
export function subscribeTraceFeEvents(listener: TraceFeListener): () => void {
  feListeners.add(listener);
  return () => {
    feListeners.delete(listener);
  };
}

export function noteTraceEvent(event: string, opts?: TraceFeEventOpts): void {
  if (!enabled) return;
  const runId = opts?.runId ?? activeRunId;
  for (const listener of feListeners) {
    try {
      listener(event, opts);
    } catch {
      /* ignore subscriber errors */
    }
  }
  void invoke('tile_gen_trace_fe_event', {
    req: {
      event,
      runId: runId ?? undefined,
      layerId: opts?.layerId,
      docGen: opts?.docGen,
      layerGen: opts?.layerGen,
      level: opts?.level,
      x: opts?.x,
      y: opts?.y,
      gen: opts?.gen,
      details: opts?.details ?? {},
      tsMs: epochMs(),
    },
  }).catch(() => {});
}

/** Emulate slider onChange through the same debounce path when possible. */
export function noteUiParamChange(params: Record<string, unknown>): void {
  noteTraceEvent('ui_param_change', { details: { params } });
}

export function noteIpcUpdateFilter(params: Record<string, unknown>): void {
  ipcCallSeq += 1;
  noteTraceEvent('ipc_update_filter', {
    details: { params, call: ipcCallSeq },
  });
}

export async function busySnapshot(): Promise<{
  silent: boolean;
  queued: number;
  inflight: number;
  full_document_busy: number;
  doc_gen: number | null;
  pending_preview_refresh: unknown;
} | null> {
  if (!enabled) return null;
  try {
    return await invoke('tile_gen_trace_busy_snapshot');
  } catch {
    return null;
  }
}

/** Wait until backend reports silence, then settle 1500ms. */
export async function waitForSilence(
  timeoutMs = 30_000,
  settleMs = 1500,
): Promise<'ok' | 'STUCK_BUSY'> {
  const start = performance.now();
  while (performance.now() - start < timeoutMs) {
    const snap = await busySnapshot();
    if (snap?.silent) {
      await new Promise((r) => setTimeout(r, settleMs));
      const again = await busySnapshot();
      if (again?.silent) return 'ok';
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  return 'STUCK_BUSY';
}

/**
 * Wait until `vp_computed` hash stops changing for `settleMs`.
 * BE silence alone is not enough — FE may still pan/resize/debounce.
 * No events for `settleMs` also counts as stable (already idle).
 */
export async function waitForFeViewportStable(
  settleMs = 1500,
  timeoutMs = 15_000,
): Promise<'ok' | 'STUCK_MOVING'> {
  let lastHash: string | null = null;
  let lastChange = performance.now();
  const unsub = subscribeTraceFeEvents((event, opts) => {
    if (event !== 'vp_computed') return;
    const h = opts?.details?.vp_hash;
    if (typeof h !== 'string') return;
    if (h !== lastHash) {
      lastHash = h;
      lastChange = performance.now();
    }
  });
  const start = performance.now();
  try {
    while (performance.now() - start < timeoutMs) {
      if (performance.now() - lastChange >= settleMs) {
        return 'ok';
      }
      await new Promise((r) => setTimeout(r, 50));
    }
    return 'STUCK_MOVING';
  } finally {
    unsub();
  }
}

export type TraceCanvasHooks = {
  getVisibleKeys: () => string[];
  getViewportLevel: () => number;
  getTileRev?: () => number;
  /** Displayed document gen per tile key (`level/x/y`). */
  getTileGens?: () => Record<string, number>;
  /** Session max known doc_gen from wakes / progress / headers. */
  getMaxKnownGen?: () => number;
  sampleCenterPixelOpaque?: (key: string) => boolean | null;
};

export type TraceCameraApi = {
  setZoom: (z: number) => void;
  panBy: (dx: number, dy: number) => void;
  fit: () => void;
  getZoom: () => number;
  getVisibleTiles: () => Array<{ level: number; x: number; y: number }>;
  getLevel: () => number;
  getTileRev: () => number;
  getTileGens?: () => Record<string, number>;
  getMaxKnownGen?: () => number;
  /** Push current camera to BE with zero debounce (quiet-check sync). */
  flushViewport?: () => void;
  /** Place the visible set over the document bottom-right corner (Z3 cold). */
  panToBottomRight?: () => void;
};

declare global {
  interface Window {
    __ditherTileGenTrace?: {
      enabled: () => boolean;
      beginRun: typeof beginTraceRun;
      note: typeof noteTraceEvent;
      waitForSilence: typeof waitForSilence;
      busySnapshot: typeof busySnapshot;
      canvas?: TraceCanvasHooks;
      camera?: TraceCameraApi;
    };
  }
}

/** Expose a small stress hook under a flag for programmatic series. */
export function installTraceStressHook(): void {
  if (typeof window === 'undefined' || !enabled) return;
  window.__ditherTileGenTrace = {
    ...(window.__ditherTileGenTrace ?? {}),
    enabled: () => enabled,
    beginRun: beginTraceRun,
    note: noteTraceEvent,
    waitForSilence,
    busySnapshot,
  };
}

export function registerTraceCanvasHooks(hooks: TraceCanvasHooks): void {
  if (typeof window === 'undefined' || !enabled) return;
  installTraceStressHook();
  if (window.__ditherTileGenTrace) {
    window.__ditherTileGenTrace.canvas = hooks;
  }
}
