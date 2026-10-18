/**
 * Visible-set divergence checker (viewport logger spec).
 * Observation only — no production behavior changes.
 */

import { invoke } from '@tauri-apps/api/core';
import { tileRangeFromCoords, type TileRange } from './vpHash';
import {
  busySnapshot,
  noteTraceEvent,
  tileGenTraceEnabled,
  waitForFeViewportStable,
  waitForSilence,
} from './tileGenTrace';

export type VpViolationClass =
  | 'VP_NOT_SENT'
  | 'VP_LOST_IN_FLIGHT'
  | 'VP_MISMATCH_FE_BE'
  | 'VP_STALE_SCHEDULE'
  | 'TILE_NOT_REQUESTED'
  | 'TILE_NOT_SERVED'
  | 'TILE_SERVED_NOT_DRAWN'
  | 'DRAWN_NOT_VISIBLE'
  | 'WORKER_SPIN'
  | 'TIME_TO_FRESH'
  | 'VISIBLE_HOLES'
  | 'INJECT_SELFCHECK_FAILED';

export type VpViolation = {
  class: VpViolationClass;
  tileKey?: string;
  detail: string;
  feHash?: string;
  beHash?: string;
  scheduleHash?: string;
};

export type FeVpSnapshot = {
  level: number;
  vp_hash: string;
  tiles: Array<{ level: number; x: number; y: number }>;
  range: TileRange;
  tileRev: number;
};

export type SeriesTileTrace = {
  requested: Set<string>;
  response200: Set<string>;
  responseStuck: Set<string>;
  /** Any draw in the series (coverage for TILE_NOT_REQUESTED). */
  drawn: Set<string>;
  /** Draws while `lastFeHash` matched the current camera hash (quiet DRAWN_*). */
  drawnAtCurrentHash: Set<string>;
  dropped: Map<string, string>;
  sentSeqs: number[];
  receivedSeqs: number[];
  lastFeHash: string | null;
  lastSentHash: string | null;
  lastBeHash: string | null;
  lastScheduleHash: string | null;
  feChangedWithoutSend: boolean;
  /** `vp_not_sent` with reason `inject_skip` observed in this series. */
  injectSkipSeen: boolean;
};

export function emptySeriesTrace(): SeriesTileTrace {
  return {
    requested: new Set(),
    response200: new Set(),
    responseStuck: new Set(),
    drawn: new Set(),
    drawnAtCurrentHash: new Set(),
    dropped: new Map(),
    sentSeqs: [],
    receivedSeqs: [],
    lastFeHash: null,
    lastSentHash: null,
    lastBeHash: null,
    lastScheduleHash: null,
    feChangedWithoutSend: false,
    injectSkipSeen: false,
  };
}

export type SpinOffender = {
  tile_key: string;
  level: number;
  x: number;
  y: number;
  doc_gen: number;
  dequeues_without_compute: number;
  requeue_attempt: number;
};

/** Pure WORKER_SPIN check (N=20 dequeues without compute, or attempt > N). */
export function checkWorkerSpin(
  offenders: SpinOffender[],
  threshold = 20,
): VpViolation[] {
  return offenders
    .filter(
      (o) =>
        o.dequeues_without_compute > threshold || o.requeue_attempt > threshold,
    )
    .map((o) => ({
      class: 'WORKER_SPIN' as const,
      tileKey: o.tile_key,
      detail: `dequeues_without_compute=${o.dequeues_without_compute} requeue_attempt=${o.requeue_attempt} (threshold=${threshold}) gen=${o.doc_gen}`,
    }));
}

export async function backendSpinSnapshot(): Promise<{
  threshold: number;
  offenders: SpinOffender[];
} | null> {
  if (!tileGenTraceEnabled()) return null;
  try {
    const v = await invoke<{
      ok: boolean;
      threshold?: number;
      offenders?: SpinOffender[];
    }>('tile_gen_trace_spin_snapshot');
    if (!v?.ok) return null;
    return {
      threshold: v.threshold ?? 20,
      offenders: v.offenders ?? [],
    };
  } catch {
    return null;
  }
}

/** Pure checker used by unit tests and live runs. */
export function checkViewportInvariant(args: {
  fe: FeVpSnapshot;
  beHash: string | null;
  scheduleHash: string | null;
  trace: SeriesTileTrace;
  /**
   * Tile coverage classes only apply after true quiet (spec §4/§6).
   * When false (STUCK_BUSY / STUCK_MOVING), only VP_* classes are reported.
   */
  quietOk?: boolean;
}): VpViolation[] {
  const out: VpViolation[] = [];
  const feKeys = new Set(args.fe.tiles.map((t) => `${t.level}/${t.x}/${t.y}`));
  const quietOk = args.quietOk !== false;

  if (args.trace.feChangedWithoutSend) {
    out.push({
      class: 'VP_NOT_SENT',
      detail: 'FE visible set changed without vp_sent_ipc',
      feHash: args.fe.vp_hash,
    });
  }

  for (const seq of args.trace.sentSeqs) {
    if (!args.trace.receivedSeqs.includes(seq)) {
      out.push({
        class: 'VP_LOST_IN_FLIGHT',
        detail: `vp_sent_ipc seq=${seq} has no vp_ipc_ack`,
        feHash: args.fe.vp_hash,
      });
    }
  }

  if (args.beHash != null && args.beHash !== args.fe.vp_hash) {
    out.push({
      class: 'VP_MISMATCH_FE_BE',
      detail: 'last FE vp_hash != BE vp_state_applied',
      feHash: args.fe.vp_hash,
      beHash: args.beHash,
    });
  }

  if (
    args.scheduleHash != null &&
    args.beHash != null &&
    args.scheduleHash !== args.beHash
  ) {
    out.push({
      class: 'VP_STALE_SCHEDULE',
      detail: 'last schedule_enter vp_hash != BE state',
      beHash: args.beHash,
      scheduleHash: args.scheduleHash,
    });
  }

  if (!quietOk) {
    return out;
  }

  for (const key of feKeys) {
    // Cache hits redraw without a new fetch; drawn covers "arrived on canvas".
    if (!args.trace.requested.has(key) && !args.trace.drawn.has(key)) {
      out.push({
        class: 'TILE_NOT_REQUESTED',
        tileKey: key,
        detail: 'visible tile never requested (and not drawn)',
        feHash: args.fe.vp_hash,
      });
      continue;
    }
    if (!args.trace.requested.has(key)) {
      // Drawn from an existing bitmap — skip request/response chain checks.
      continue;
    }
    if (args.trace.responseStuck.has(key) && !args.trace.response200.has(key)) {
      out.push({
        class: 'TILE_NOT_SERVED',
        tileKey: key,
        detail: '202/404 persisted until quiet',
        feHash: args.fe.vp_hash,
      });
      continue;
    }
    if (args.trace.response200.has(key) && !args.trace.drawn.has(key)) {
      const drop = args.trace.dropped.get(key);
      out.push({
        class: 'TILE_SERVED_NOT_DRAWN',
        tileKey: key,
        detail: drop ? `dropped: ${drop}` : '200 without tile_drawn',
        feHash: args.fe.vp_hash,
      });
    }
  }

  // Same-level draws under the final camera hash, outside the quiet visible set.
  // Coarser LOD / ancestor fills are requested+drawn on purpose — skip other levels.
  const levelPrefix = `${args.fe.level}/`;
  for (const key of args.trace.drawnAtCurrentHash) {
    if (!key.startsWith(levelPrefix)) continue;
    if (feKeys.has(key)) continue;
    if (!args.trace.requested.has(key)) continue;
    out.push({
      class: 'DRAWN_NOT_VISIBLE',
      tileKey: key,
      detail: 'requested same-level tile drawn outside last FE visible set',
      feHash: args.fe.vp_hash,
    });
  }

  return out;
}

export async function probeCacheTile(
  level: number,
  x: number,
  y: number,
  layerId?: number,
): Promise<Record<string, unknown> | null> {
  if (!tileGenTraceEnabled()) return null;
  try {
    return await invoke('tile_gen_trace_cache_probe', {
      req: { level, x, y, layerId },
    });
  } catch {
    return null;
  }
}

export async function backendViewportSnapshot(): Promise<{
  vp_hash: string;
  level: number;
  tiles: Array<{ level: number; x: number; y: number }>;
} | null> {
  if (!tileGenTraceEnabled()) return null;
  try {
    const v = await invoke<{
      ok: boolean;
      vp_hash?: string;
      level?: number;
      tiles?: Array<{ level: number; x: number; y: number }>;
    }>('tile_gen_trace_viewport_snapshot');
    if (!v?.ok || !v.vp_hash || v.level == null) return null;
    return { vp_hash: v.vp_hash, level: v.level, tiles: v.tiles ?? [] };
  } catch {
    return null;
  }
}

export type QuietFreshnessMetrics = {
  /** Wall ms from `freshnessT0` to quiet with zero visible holes; null if not fresh. */
  timeToFreshMs: number | null;
  /** Visible tiles lacking a draw at the current camera hash after silence. */
  holes: number;
  visible: number;
  drawnAtCurrent: number;
};

export async function runQuietThenCheck(args: {
  getFeSnapshot: () => FeVpSnapshot;
  trace: SeriesTileTrace;
  /** Optional: flush FE camera → BE before comparing hashes. */
  flushViewport?: () => void;
  /** performance.now() when the freshness clock started (e.g. final zoom). */
  freshnessT0?: number;
  /** Flag TIME_TO_FRESH when wall ms exceeds this (optional). */
  timeToFreshBudgetMs?: number;
}): Promise<{
  silence: 'ok' | 'STUCK_BUSY' | 'STUCK_MOVING';
  violations: VpViolation[];
  cache: Record<string, unknown>[];
  freshness: QuietFreshnessMetrics;
}> {
  // CPU floyd_steinberg after zoom-out can keep the queue busy well past 30s.
  const beSilence = await waitForSilence(120_000, 1500);
  const feStable = await waitForFeViewportStable();
  args.flushViewport?.();
  // Allow the zero-debounce IPC + BE apply to land.
  await new Promise((r) => setTimeout(r, 50));
  const feStableAfterFlush = await waitForFeViewportStable(400, 5_000);

  const silence =
    beSilence !== 'ok'
      ? beSilence
      : feStable !== 'ok' || feStableAfterFlush !== 'ok'
        ? 'STUCK_MOVING'
        : 'ok';

  const fe = args.getFeSnapshot();
  const be = await backendViewportSnapshot();
  const violations: VpViolation[] = checkViewportInvariant({
    fe,
    beHash: be?.vp_hash ?? args.trace.lastBeHash,
    scheduleHash: args.trace.lastScheduleHash,
    trace: args.trace,
    quietOk: silence === 'ok',
  });

  // WORKER_SPIN is independent of quiet — a spinning worker must always fail.
  const spin = await backendSpinSnapshot();
  if (spin) {
    violations.push(...checkWorkerSpin(spin.offenders, spin.threshold));
  }

  const cache: Record<string, unknown>[] = [];
  for (const v of violations) {
    if (!v.tileKey) continue;
    const [ls, xs, ys] = v.tileKey.split('/');
    const probe = await probeCacheTile(Number(ls), Number(xs), Number(ys));
    if (probe) {
      cache.push({ tileKey: v.tileKey, ...probe });
      noteTraceEvent('cache_probe', {
        level: Number(ls),
        x: Number(xs),
        y: Number(ys),
        details: probe,
      });
    }
  }

  const visibleKeys = fe.tiles.map((t) => `${t.level}/${t.x}/${t.y}`);
  const holes = visibleKeys.filter((k) => !args.trace.drawnAtCurrentHash.has(k)).length;
  const drawnAtCurrent = visibleKeys.filter((k) =>
    args.trace.drawnAtCurrentHash.has(k),
  ).length;
  const timeToFreshMs =
    silence === 'ok' && holes === 0 && args.freshnessT0 != null
      ? performance.now() - args.freshnessT0
      : null;
  const freshness: QuietFreshnessMetrics = {
    timeToFreshMs,
    holes,
    visible: visibleKeys.length,
    drawnAtCurrent,
  };

  if (holes > 0 && silence === 'ok') {
    violations.push({
      class: 'VISIBLE_HOLES',
      detail: `holes=${holes}/${visibleKeys.length} after silence`,
    });
  }
  if (
    args.timeToFreshBudgetMs != null &&
    (timeToFreshMs == null || timeToFreshMs > args.timeToFreshBudgetMs)
  ) {
    violations.push({
      class: 'TIME_TO_FRESH',
      detail:
        timeToFreshMs == null
          ? `not fresh after silence (holes=${holes}, silence=${silence}); budget=${args.timeToFreshBudgetMs}ms`
          : `time_to_fresh=${timeToFreshMs.toFixed(0)}ms > budget=${args.timeToFreshBudgetMs}ms`,
    });
  }

  noteTraceEvent('vp_invariant_result', {
    details: {
      silence,
      fe_stable: feStable,
      violation_count: violations.length,
      classes: violations.map((v) => v.class),
      fe_hash: fe.vp_hash,
      be_hash: be?.vp_hash ?? null,
      last_sent_hash: args.trace.lastSentHash,
      busy: await busySnapshot(),
      time_to_fresh_ms: timeToFreshMs,
      holes,
      visible: visibleKeys.length,
    },
  });

  return { silence, violations, cache, freshness };
}

export function feSnapshotFromTiles(
  level: number,
  tiles: Array<{ level: number; x: number; y: number }>,
  tileRev: number,
): FeVpSnapshot {
  const range = tileRangeFromCoords(level, tiles);
  return {
    level,
    vp_hash: range.vp_hash,
    tiles,
    range,
    tileRev,
  };
}

/** Fold a live FE `noteTraceEvent` into a per-series ledger. */
export function ingestFeEventIntoSeriesTrace(
  trace: SeriesTileTrace,
  event: string,
  opts?: {
    level?: number;
    x?: number;
    y?: number;
    details?: Record<string, unknown>;
  },
): void {
  const level = opts?.level;
  const x = opts?.x;
  const y = opts?.y;
  const key =
    level != null && x != null && y != null ? `${level}/${x}/${y}` : null;
  const details = opts?.details ?? {};

  if (event === 'tile_requested' && key) {
    trace.requested.add(key);
    return;
  }
  if (event === 'tile_response' && key) {
    const status = details.status;
    if (status === 200) {
      trace.response200.add(key);
      trace.responseStuck.delete(key);
    } else if (status === 202 || status === 404) {
      if (!trace.response200.has(key)) trace.responseStuck.add(key);
    }
    return;
  }
  if (event === 'tile_drawn' && key) {
    trace.drawn.add(key);
    trace.drawnAtCurrentHash.add(key);
    return;
  }
  if (event === 'tile_dropped' && key) {
    const reason =
      typeof details.reason === 'string' ? details.reason : 'dropped';
    trace.dropped.set(key, reason);
    return;
  }
  if (event === 'vp_not_sent') {
    if (details.reason === 'inject_skip') {
      trace.injectSkipSeen = true;
      trace.feChangedWithoutSend = true;
    }
    return;
  }
  if (event === 'vp_sent_ipc') {
    const seq = details.trace_seq;
    if (typeof seq === 'number') trace.sentSeqs.push(seq);
    if (typeof details.vp_hash === 'string') {
      trace.lastSentHash = details.vp_hash;
      trace.lastFeHash = details.vp_hash;
    }
    return;
  }
  if (event === 'vp_ipc_ack') {
    const seq = details.trace_seq;
    if (typeof seq === 'number' && !trace.receivedSeqs.includes(seq)) {
      trace.receivedSeqs.push(seq);
    }
    return;
  }
  if (event === 'vp_computed' && typeof details.vp_hash === 'string') {
    if (trace.lastFeHash != null && details.vp_hash !== trace.lastFeHash) {
      trace.drawnAtCurrentHash.clear();
    }
    trace.lastFeHash = details.vp_hash;
  }
}
