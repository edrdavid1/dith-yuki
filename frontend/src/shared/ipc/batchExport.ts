import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type BatchOutputFormat = 'png' | 'png8';

export interface BatchExportRequest {
  doc_id: number;
  layer_id?: number | null;
  input_dir: string;
  output_dir: string;
  name_template?: string;
  format?: BatchOutputFormat;
  lock_pattern_phase?: boolean;
}

export interface BatchExportProgress {
  done: number;
  total: number;
  current_input: string;
  last_error: string | null;
  stage: string;
}

export interface BatchJobResult {
  input: string;
  output: string | null;
  error: string | null;
}

export interface BatchExportSummary {
  succeeded: number;
  failed: number;
  cancelled: boolean;
  results: BatchJobResult[];
}

export async function batchExportRun(
  req: BatchExportRequest
): Promise<BatchExportSummary> {
  return invoke<BatchExportSummary>('batch_export_run', { req });
}

export async function batchExportCancel(): Promise<void> {
  return invoke<void>('batch_export_cancel');
}

export async function onBatchExportProgress(
  handler: (progress: BatchExportProgress) => void
): Promise<UnlistenFn> {
  return listen<BatchExportProgress>('batch_export_progress', (event) =>
    handler(event.payload)
  );
}
