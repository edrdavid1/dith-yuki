/**
 * Shared ed-progress state for TileCanvas re-fetch wakeups and the UI indicator.
 */

export type EdProgressSnapshot = {
  level: number;
  done: number;
  total: number;
  docGen: number;
  sourceId: string;
  docId: number;
  parallelEdPasses: number;
  /** performance.now() when last updated */
  updatedAt: number;
};

type Listener = (snap: EdProgressSnapshot | null) => void;

let latest: EdProgressSnapshot | null = null;
const listeners = new Set<Listener>();

export function getEdProgress(): EdProgressSnapshot | null {
  return latest;
}

export function setEdProgress(snap: EdProgressSnapshot | null): void {
  latest = snap;
  for (const l of listeners) l(snap);
}

export function subscribeEdProgress(listener: Listener): () => void {
  listeners.add(listener);
  listener(latest);
  return () => {
    listeners.delete(listener);
  };
}

/** Reasons that must not use the 5-retry ceiling; wake via events + slow safety. */
export const EVENT_DRIVEN_PENDING_REASONS = new Set([
  'ed_prefix',
  'waiting_children',
  'computing',
]);

export const PENDING_SAFETY_RETRY_MS = 1500;
