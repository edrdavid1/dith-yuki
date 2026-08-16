import { listen, emit, emitTo, type UnlistenFn, type Event } from '@tauri-apps/api/event';
import type { SelectionDto } from './selection';
import type { PanelInfo, PanelStateSnapshot } from '../../types/panels';
import type { ColorLabDraftSnapshot } from '../../features/color-lab/types';

export interface DocumentChangedPayload {
  kind: string;
  layer_id?: number | null;
}

export type SelectionChangedPayload = SelectionDto;

export type PanelStateChangedPayload = PanelStateSnapshot | PanelInfo[];

export type ColorLabDraftChangedPayload = ColorLabDraftSnapshot;

export async function onDocumentChanged(
  handler: (event: Event<DocumentChangedPayload>) => void
): Promise<UnlistenFn> {
  return listen<DocumentChangedPayload>('document-changed', handler);
}

export async function onSelectionChanged(
  handler: (event: Event<SelectionChangedPayload>) => void
): Promise<UnlistenFn> {
  return listen<SelectionChangedPayload>('selection-changed', handler);
}

export async function onPanelStateChanged(
  handler: (event: Event<PanelStateChangedPayload>) => void
): Promise<UnlistenFn> {
  return listen<PanelStateChangedPayload>('panel-state-changed', handler);
}

export async function emitPanelStateChanged(): Promise<void> {
  return emit('panel-state-changed');
}

export async function onColorLabDraftChanged(
  handler: (event: Event<ColorLabDraftChangedPayload>) => void
): Promise<UnlistenFn> {
  return listen<ColorLabDraftChangedPayload>('color-lab-draft-changed', handler);
}

export async function emitColorLabDraftChanged(
  draft: ColorLabDraftChangedPayload
): Promise<void> {
  // `emit` is webview-local in Tauri 2; floating Color Lab / Effects / Preview
  // each have their own store and must all receive the draft.
  return emitTo('any', 'color-lab-draft-changed', draft);
}

export interface PaletteBindingPayload {
  lastCreatedId: number | null;
}

export async function onPaletteBindingChanged(
  handler: (event: Event<PaletteBindingPayload>) => void
): Promise<UnlistenFn> {
  return listen<PaletteBindingPayload>('palette-binding-changed', handler);
}

export async function emitPaletteBindingChanged(
  lastCreatedId: number | null
): Promise<void> {
  return emitTo('any', 'palette-binding-changed', { lastCreatedId });
}

export async function emitPaletteChanged(): Promise<void> {
  return emitTo('any', 'palette-changed', {});
}

export interface DockAffinityEvent {
  panelId: string;
  armed: boolean;
  insertIndex: number | null;
  /** Armed dock side when armed; null when disarmed. */
  side: 'left' | 'right' | null;
}

export async function onDockAffinity(
  handler: (event: Event<DockAffinityEvent>) => void
): Promise<UnlistenFn> {
  return listen<DockAffinityEvent>('dock-affinity', handler);
}
