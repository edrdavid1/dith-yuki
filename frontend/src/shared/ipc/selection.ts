import { invoke } from '@tauri-apps/api/core';

export interface SelectionDto {
  selected_layer_id: number | null;
  selected_filter_id: string | null;
}

export async function getSelection(): Promise<SelectionDto> {
  return invoke<SelectionDto>('get_selection');
}

export async function setSelection(
  layerId: number | null,
  filterId: string | null
): Promise<void> {
  return invoke<void>('set_selection', { layerId, filterId });
}
