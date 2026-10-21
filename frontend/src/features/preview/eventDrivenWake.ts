/**
 * Event-driven tile wake + generation gates (H4 Step B).
 *
 * When a wake arrives while a fetch is already in flight, do **not** start a
 * second request — mark `needsRefetch` and collapse to ≤1 follow-up after the
 * in-flight response completes.
 */

export type EventDrivenWakeDecision =
  | { action: 'fetch' }
  | { action: 'needs_refetch'; reason: 'inflight'; cause: string };

export type EventDrivenPendingState = {
  inflight: boolean;
  needsRefetch?: boolean;
  /** At most one collapsed retry after an inflight wake. */
  refetchDone?: boolean;
  maxKnownGen?: number;
};

/**
 * Decide whether to post a wake fetch now or coalesce into needsRefetch.
 */
export function decideEventDrivenWake(
  entry: EventDrivenPendingState | undefined,
  cause: string,
): EventDrivenWakeDecision {
  if (entry?.inflight) {
    return { action: 'needs_refetch', reason: 'inflight', cause };
  }
  return { action: 'fetch' };
}

/** Raise the known ceiling for a tile / session. */
export function bumpMaxKnownGen(current: number, incoming: number | null | undefined): number {
  if (incoming == null || !Number.isFinite(incoming)) return current;
  return Math.max(current, incoming);
}

/**
 * Discard a decode that is older than what we already display for this tile.
 * (Out-of-order responses after a newer gen was drawn.)
 */
export function shouldDiscardStaleGen(
  responseGen: number | null | undefined,
  displayedGen: number | null | undefined,
): boolean {
  if (responseGen == null || displayedGen == null) return false;
  return responseGen < displayedGen;
}

/**
 * After an inflight fetch completes: one collapsed retry if the response is
 * behind the max *ready* gen (or needsRefetch was set while waiting).
 *
 * `maxKnownGen` must only track gens observed as ready (tile-ready / 200),
 * never an optimistic filter-apply / 202 target — that causes request storms.
 */
export function shouldRetryForMaxGen(args: {
  responseGen: number | null | undefined;
  maxKnownGen: number;
  needsRefetch: boolean;
  refetchDone: boolean;
}): boolean {
  if (args.refetchDone) return false;
  if (args.needsRefetch) return true;
  if (args.responseGen == null) return false;
  return args.responseGen < args.maxKnownGen;
}

/**
 * Collapsed retry already spent, but the body is still behind a ready gen
 * (or marked stale quality). Safety timer must be allowed to recover —
 * do not treat as final until pan/zoom.
 */
export function collapsedRetryStillStale(args: {
  responseGen: number | null | undefined;
  maxKnownGen: number;
  refetchDone: boolean;
  quality?: string | null;
}): boolean {
  if (!args.refetchDone) return false;
  const q = args.quality;
  if (q === 'stale' || q === 'upsample' || q === 'pyramid') return true;
  if (args.responseGen == null) return args.maxKnownGen > 0;
  return args.responseGen < args.maxKnownGen;
}

/**
 * Parse `X-Tile-Gen` → content generation for FE maxKnown / safety.
 * Shape: `doc_gen.layer_gen` (legacy) or `doc_gen.layer_gen.content_gen` (Step B).
 * Prefer the last segment when present so palette-only bumps are visible.
 */
export function parseTileGenHeader(raw: string | null | undefined): number | null {
  if (!raw) return null;
  const parts = raw.split('.');
  const contentPart = parts.length >= 3 ? parts[parts.length - 1] : parts[0];
  const n = Number(contentPart);
  return Number.isFinite(n) ? n : null;
}
