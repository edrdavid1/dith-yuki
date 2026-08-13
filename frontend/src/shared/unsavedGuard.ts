export type UnsavedGuardChoice = 'save' | 'discard' | 'cancel';

export function projectBasename(path: string | null | undefined): string {
  if (!path) return 'Untitled';
  const name = path.split(/[/\\]/).pop();
  return name && name.length > 0 ? name : 'Untitled';
}

/**
 * Shared close / replace / update prompt. Callers run Save themselves on `'save'`.
 */
export async function confirmUnsavedIfNeeded(opts: {
  hasDocument: boolean;
  dirty: boolean;
  prompt: () => Promise<UnsavedGuardChoice>;
  save: () => Promise<boolean>;
}): Promise<boolean> {
  if (!opts.hasDocument || !opts.dirty) return true;
  const choice = await opts.prompt();
  if (choice === 'cancel') return false;
  if (choice === 'discard') return true;
  return opts.save();
}
