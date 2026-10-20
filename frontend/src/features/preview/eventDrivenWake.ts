/**
 * Pure helpers for event-driven tile wake (tile-ready / ed-progress / safety).
 * Observation helpers for H4: wake skipped while a fetch is already in flight.
 */

export type EventDrivenWakeDecision =
  | { action: 'fetch' }
  | { action: 'skip'; reason: 'inflight'; cause: string };

/**
 * Decide whether to post a wake fetch for a pending tile.
 * When `inflight` is already true, a later tile-ready/ed-progress must not
 * start a duplicate fetch — callers should emit `wake_skipped_inflight`.
 */
export function decideEventDrivenWake(
  entry: { inflight: boolean } | undefined,
  cause: string,
): EventDrivenWakeDecision {
  if (entry?.inflight) {
    return { action: 'skip', reason: 'inflight', cause };
  }
  return { action: 'fetch' };
}
