import { useSyncExternalStore } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { EXPORT_JOB_COPY } from './copy/exportJobs';

export type ExportJobKind = 'print' | 'batch';

interface ExportJobChangedPayload {
  kind: string;
  active: boolean;
  busy_kind: string | null;
}

let busy: ExportJobKind | null = null;
const listeners = new Set<() => void>();
let backendListen: Promise<UnlistenFn> | null = null;

function notify() {
  listeners.forEach((l) => l());
}

function setBusy(next: ExportJobKind | null) {
  if (busy === next) return;
  busy = next;
  notify();
}

/** Subscribe to backend `export-job-changed` once for the process. */
export function ensureExportJobListener(): void {
  if (backendListen) return;
  backendListen = listen<ExportJobChangedPayload>('export-job-changed', (event) => {
    const payload = event.payload;
    if (payload.active && (payload.kind === 'print' || payload.kind === 'batch')) {
      setBusy(payload.kind);
      return;
    }
    if (payload.busy_kind === 'print' || payload.busy_kind === 'batch') {
      setBusy(payload.busy_kind);
      return;
    }
    setBusy(null);
  });
}

export function getExportJobBusy(): ExportJobKind | null {
  return busy;
}

export function subscribeExportJobBusy(onStoreChange: () => void): () => void {
  ensureExportJobListener();
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

export function useExportJobBusy(): ExportJobKind | null {
  return useSyncExternalStore(subscribeExportJobBusy, getExportJobBusy, () => null);
}

export function parseJobBusyError(err: unknown): ExportJobKind | null {
  const text = err instanceof Error ? err.message : String(err ?? '');
  if (text.includes('job_busy:print')) return 'print';
  if (text.includes('job_busy:batch')) return 'batch';
  return null;
}

export function jobBusyMessage(kind: ExportJobKind): string {
  return kind === 'print' ? EXPORT_JOB_COPY.busyToastPrint : EXPORT_JOB_COPY.busyToastBatch;
}

export { EXPORT_JOB_COPY };
