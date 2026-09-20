export type UnsavedGuardChoice = 'save' | 'discard' | 'cancel';

/** Result of the multi-document quit dialog (one prompt for N dirty tabs). */
export type UnsavedMultiChoice =
  | { kind: 'cancel' }
  | { kind: 'discard-all' }
  | { kind: 'save-selected'; selectedIds: number[] };

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
 * Quit / window close: one prompt when several tabs are dirty; single-doc path when one.
 * Cancel or any failed save aborts the whole operation.
 */
export async function confirmUnsavedDocuments(opts: {
  documents: UnsavedDocumentRef[];
  promptSingle: (doc: UnsavedDocumentRef) => Promise<UnsavedGuardChoice>;
  promptMulti: (docs: UnsavedDocumentRef[]) => Promise<UnsavedMultiChoice>;
  save: (doc: UnsavedDocumentRef) => Promise<boolean>;
}): Promise<boolean> {
  const dirty = opts.documents.filter((d) => d.dirty);
  if (dirty.length === 0) return true;

  if (dirty.length === 1) {
    const doc = dirty[0]!;
    return confirmUnsavedIfNeeded({
      hasDocument: true,
      dirty: true,
      prompt: () => opts.promptSingle(doc),
      save: () => opts.save(doc),
    });
  }

  const choice = await opts.promptMulti(dirty);
  if (choice.kind === 'cancel') return false;
  if (choice.kind === 'discard-all') return true;

  const selected = new Set(choice.selectedIds);
  for (const doc of dirty) {
    if (!selected.has(doc.id)) continue;
    const saved = await opts.save(doc);
    if (!saved) return false;
  }
  return true;
}
