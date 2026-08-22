export type UnsavedGuardChoice = 'save' | 'discard' | 'cancel';

export function projectBasename(path: string | null | undefined): string {
  if (!path) return 'Untitled';
  const name = path.split(/[/\\]/).pop();
  return name && name.length > 0 ? name : 'Untitled';
}

/** One open document that may need Save / Don’t Save / Cancel (VS Code / Photoshop). */
export interface UnsavedDocumentRef {
  id: number;
  dirty: boolean;
  path: string | null;
  /** Tab title fallback when path is null. */
  title?: string;
}

export function displayNameForUnsaved(doc: UnsavedDocumentRef): string {
  if (doc.path) return projectBasename(doc.path);
  if (doc.title && doc.title.length > 0) return doc.title;
  return 'Untitled';
}

/**
 * Shared close / replace prompt for a single document.
 * Callers run Save themselves on `'save'`.
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

/**
 * VS Code / Photoshop quit: prompt each dirty document in order.
 * Cancel on any document aborts the whole operation (quit / close window).
 */
export async function confirmUnsavedDocuments(opts: {
  documents: UnsavedDocumentRef[];
  /** Show dialog for this document; basename is already set by the UI layer. */
  promptFor: (doc: UnsavedDocumentRef) => Promise<UnsavedGuardChoice>;
  save: (doc: UnsavedDocumentRef) => Promise<boolean>;
}): Promise<boolean> {
  const dirty = opts.documents.filter((d) => d.dirty);
  for (const doc of dirty) {
    const choice = await opts.promptFor(doc);
    if (choice === 'cancel') return false;
    if (choice === 'discard') continue;
    const saved = await opts.save(doc);
    if (!saved) return false;
  }
  return true;
}
