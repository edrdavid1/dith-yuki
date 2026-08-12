/** Normalize Tauri/IPC thrown values into a string message. */
export function formatIpcError(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

/** Minimum mutate-path logging so failures are never fully silent. */
export function logIpcError(context: string, err: unknown): void {
  console.error(`[ipc] ${context}:`, formatIpcError(err), err);
}
