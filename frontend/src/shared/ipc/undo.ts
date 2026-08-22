import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn, type Event } from '@tauri-apps/api/event';

export interface UndoStateDto {
  can_undo: boolean;
  can_redo: boolean;
  doc_id?: number;
}

export async function undo(docId: number): Promise<UndoStateDto> {
  return invoke<UndoStateDto>('undo', { docId });
}

export async function redo(docId: number): Promise<UndoStateDto> {
  return invoke<UndoStateDto>('redo', { docId });
}

export async function onUndoStateChanged(
  handler: (event: Event<UndoStateDto>) => void
): Promise<UnlistenFn> {
  return listen<UndoStateDto>('undo-state-changed', handler);
}

export interface DirtyDto {
  dirty: boolean;
  doc_id?: number;
}

export async function isDocumentDirty(docId?: number | null): Promise<boolean> {
  return invoke<boolean>('is_document_dirty', {
    docId: docId ?? null,
  });
}

export async function onDirtyChanged(
  handler: (event: Event<DirtyDto>) => void
): Promise<UnlistenFn> {
  return listen<DirtyDto>('dirty-changed', handler);
}
