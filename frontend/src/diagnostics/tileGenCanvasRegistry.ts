/**
 * Trace-only registry of tiles actually drawn to canvas + per-series counters.
 * Used by the live stress invariant checker (canvas spec).
 */

export type DrawnTileRecord = {
  gen: number | null;
  drawnAtMs: number;
  requestRev?: number;
  via202?: boolean;
};

export type TileSeriesCounters = {
  tileReady: number;
  fetchPosted: number;
  status200: number;
  status202: number;
  statusOther: number;
  dropped: number;
  dropReasons: Record<string, number>;
};

const drawn = new Map<string, DrawnTileRecord>();
const counters = new Map<string, TileSeriesCounters>();
let lastTraceEventMs = 0;
let injectRemaining = 0;
let injectDroppedKey: string | null = null;
let injectEnabled = false;
/** Spec: inject path must not ship in production bundles. */
const INJECT_BUILD =
  typeof import.meta !== 'undefined' &&
  Boolean((import.meta as { env?: { DEV?: boolean } }).env?.DEV);

function emptyCounters(): TileSeriesCounters {
  return {
    tileReady: 0,
    fetchPosted: 0,
    status200: 0,
    status202: 0,
    statusOther: 0,
    dropped: 0,
    dropReasons: {},
  };
}

function bump(key: string, field: keyof Omit<TileSeriesCounters, 'dropReasons'>, n = 1): void {
  const c = counters.get(key) ?? emptyCounters();
  c[field] = (c[field] as number) + n;
  counters.set(key, c);
}

export function noteTraceActivity(): void {
  lastTraceEventMs = performance.now();
}

export function lastTraceActivityMs(): number {
  return lastTraceEventMs;
}

export function resetSeriesRegistry(): void {
  drawn.clear();
  counters.clear();
  injectDroppedKey = null;
}

export function setInjectEnabled(on: boolean): void {
  injectEnabled = INJECT_BUILD && on;
}

export function injectEnabledFlag(): boolean {
  return injectEnabled;
}

/** Arm dropping the next N accepted tile-ready handlers (inject mode). */
export function armInjectDrop(n = 1): void {
  if (!injectEnabled) return;
  injectRemaining = n;
  injectDroppedKey = null;
}

export function takeInjectDrop(key: string): boolean {
  if (!injectEnabled || injectRemaining <= 0) return false;
  injectRemaining -= 1;
  injectDroppedKey = key;
  return true;
}

export function lastInjectDroppedKey(): string | null {
  return injectDroppedKey;
}

export function recordDrawn(
  key: string,
  gen: number | null,
  meta?: { requestRev?: number; via202?: boolean },
): void {
  drawn.set(key, {
    gen,
    drawnAtMs: performance.now(),
    requestRev: meta?.requestRev,
    via202: meta?.via202,
  });
  noteTraceActivity();
}

export function getDrawn(key: string): DrawnTileRecord | undefined {
  return drawn.get(key);
}

export function allDrawn(): Map<string, DrawnTileRecord> {
  return new Map(drawn);
}

export function recordTileReady(key: string): void {
  bump(key, 'tileReady');
  noteTraceActivity();
}

export function recordFetchPosted(key: string): void {
  bump(key, 'fetchPosted');
  noteTraceActivity();
}

export function recordHttpStatus(key: string, status: number): void {
  if (status === 200) bump(key, 'status200');
  else if (status === 202) bump(key, 'status202');
  else bump(key, 'statusOther');
  noteTraceActivity();
}

export function recordDropped(key: string, reason: string): void {
  const c = counters.get(key) ?? emptyCounters();
  c.dropped += 1;
  c.dropReasons[reason] = (c.dropReasons[reason] ?? 0) + 1;
  counters.set(key, c);
  noteTraceActivity();
}

export function getCounters(key: string): TileSeriesCounters | undefined {
  return counters.get(key);
}

export type ViolationClass =
  | 'MISSING'
  | 'STALE'
  | 'MIXED'
  | 'STUCK_202'
  | 'LOST_EVENT'
  | 'STUCK_BUSY'
  | 'STALE_DRAWN';

export type Violation = {
  class: ViolationClass;
  key: string;
  detail: string;
  drawnGen?: number | null;
  expectedGen?: number;
};

/**
 * Check visible tiles against backend layer_gen / doc_gen.
 * Spec: compare drawn gen to current layer_gen from backend.
 */
export function checkVisibleInvariant(opts: {
  visibleKeys: string[];
  expectedGen: number;
  level: number;
}): Violation[] {
  const out: Violation[] = [];
  const gensOnLevel = new Set<number>();

  for (const key of opts.visibleKeys) {
    const rec = drawn.get(key);
    const c = counters.get(key);
    if (!rec) {
      out.push({
        class: 'MISSING',
        key,
        detail: 'no drawImage registry entry after silence',
        expectedGen: opts.expectedGen,
      });
    } else if (rec.gen != null && rec.gen < opts.expectedGen) {
      out.push({
        class: 'STALE',
        key,
        detail: `drawn gen ${rec.gen} < expected ${opts.expectedGen}`,
        drawnGen: rec.gen,
        expectedGen: opts.expectedGen,
      });
      gensOnLevel.add(rec.gen);
    } else if (rec.gen != null) {
      gensOnLevel.add(rec.gen);
    }

    if (c && c.status202 > 0 && c.status200 === 0) {
      out.push({
        class: 'STUCK_202',
        key,
        detail: `had ${c.status202}×202 and 0×200`,
        expectedGen: opts.expectedGen,
      });
    }

    if (c && c.tileReady > 0 && c.fetchPosted === 0) {
      out.push({
        class: 'LOST_EVENT',
        key,
        detail: `tileReady=${c.tileReady} but fetchPosted=0`,
        expectedGen: opts.expectedGen,
      });
    }
  }

  if (gensOnLevel.size > 1) {
    out.push({
      class: 'MIXED',
      key: `level/${opts.level}`,
      detail: `gens on level: ${[...gensOnLevel].sort((a, b) => a - b).join(',')}`,
      expectedGen: opts.expectedGen,
    });
  }

  return out;
}
