import { invoke } from '@tauri-apps/api/core';

/** Mirrors `engine_registry::EffectCategory` (`rename_all = "snake_case"`). */
export type EffectCategory =
  | 'dithering'
  | 'glitch'
  | 'color_adjust'
  | 'stylize'
  | 'palette';

/** Mirrors `engine_registry::ParamField` (`tag = "type"`, `rename_all = "snake_case"`). */
export type ParamField =
  | {
      type: 'slider';
      key: string;
      label: string;
      min: number;
      max: number;
      default: number;
      step: number | null;
    }
  | {
      type: 'checkbox';
      key: string;
      label: string;
      default: boolean;
    }
  | {
      type: 'dropdown';
      key: string;
      label: string;
      options: [string, string][];
      default: string;
    };

/** Mirrors `engine_registry::AlgorithmInfo`. */
export interface AlgorithmInfo {
  id: string;
  display_name: string;
  category: EffectCategory;
  deprecated: boolean;
}

export async function getAlgorithmSchema(id: string): Promise<ParamField[]> {
  return invoke<ParamField[]>('get_algorithm_schema', { id });
}

export async function listAlgorithmsForCategory(
  category: EffectCategory
): Promise<AlgorithmInfo[]> {
  return invoke<AlgorithmInfo[]>('list_algorithms_for_category', { category });
}

export const EFFECT_CATEGORIES: EffectCategory[] = [
  'dithering',
  'glitch',
  'color_adjust',
  'stylize',
  'palette',
];
