import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn, type Event } from '@tauri-apps/api/event';

export interface UndoStateDto {
  can_undo: boolean;
  can_redo: boolean;
}

export async function undo(): Promise<UndoStateDto> {
  return invoke<UndoStateDto>('undo');
}

export async function redo(): Promise<UndoStateDto> {
  return invoke<UndoStateDto>('redo');
}

export async function onUndoStateChanged(
  handler: (event: Event<UndoStateDto>) => void
): Promise<UnlistenFn> {
  return listen<UndoStateDto>('undo-state-changed', handler);
}
