/**
 * End-to-end release→pixels profiling (`.local-doc/time-testing.md`).
 * Off until `dither_profile_enabled` returns true (`DITHER_PROFILE=1` on Rust).
 */

import { invoke } from '@tauri-apps/api/core';

let enabled = false;
let probed = false;
let runSeq = 0;
let activeRunId: number | null = null;
let pendingPointerUp: number | null = null;

function epochMs(): number {
  return performance.timeOrigin + performance.now();
}

export async function probeDitherProfile(): Promise<boolean> {
  if (probed) return enabled;
  probed = true;
  try {
    enabled = await invoke<boolean>('dither_profile_enabled');
  } catch {
    enabled = false;
  }
  return enabled;
}

export function ditherProfileEnabled(): boolean {
  return enabled;
}

export function activeProfileRunId(): number | null {
  return activeRunId;
}

/** Call from slider mouseup — starts a gesture run id. */
export function notePointerUp(): number | null {
  if (!enabled) return null;
  runSeq += 1;
  activeRunId = runSeq;
  pendingPointerUp = activeRunId;
  void invoke('dither_profile_begin', {
    req: {
      runId: activeRunId,
      scenarioId: `gesture-${activeRunId}`,
    },
  }).catch(() => {});
  void invoke('dither_profile_mark', {
    req: {
      runId: activeRunId,
      mark: 'pointer_up',
      epochMs: epochMs(),
    },
  }).catch(() => {});
  return activeRunId;
}

export function noteDebounceFired(runId?: number | null): void {
  if (!enabled) return;
  const id = runId ?? activeRunId ?? pendingPointerUp;
  if (id == null) return;
  activeRunId = id;
  void invoke('dither_profile_mark', {
    req: { runId: id, mark: 'debounce_fired', epochMs: epochMs() },
  }).catch(() => {});
}

export function noteIpcInvokeStart(runId?: number | null): void {
  if (!enabled) return;
  const id = runId ?? activeRunId;
  if (id == null) return;
  void invoke('dither_profile_mark', {
    req: { runId: id, mark: 'ipc_invoke_start', epochMs: epochMs() },
  }).catch(() => {});
}

export function noteFeMark(mark: string, runId?: number | null, generation?: number | null): void {
  if (!enabled) return;
  const id = runId ?? activeRunId;
  if (id == null) return;
  void invoke('dither_profile_mark', {
    req: {
      runId: id,
      mark,
      epochMs: epochMs(),
      generation: generation ?? undefined,
    },
  }).catch(() => {});
}

/** Attach profile_run_id to update_filter body when profiling. */
export function withProfileRunId<T extends Record<string, unknown>>(body: T): T {
  if (!enabled || activeRunId == null) return body;
  return { ...body, profile_run_id: activeRunId, profileRunId: activeRunId };
}
