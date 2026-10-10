import { invoke } from '@tauri-apps/api/core';

export type SoftProofIntent = 'relative' | 'perceptual' | 'absolute';

export interface SoftProofConfig {
  enabled: boolean;
  profile_id: string;
  intent: SoftProofIntent;
  /** Black-point compensation (Relative only); app-owned in the CMS path. */
  bpc: boolean;
  /** Sanitized label for missing-profile UX (optional). */
  profile_display_name?: string | null;
}

export interface ProofProfileInfo {
  id: string;
  name: string;
  builtin: boolean;
  has_perceptual: boolean;
  has_relative: boolean;
  has_absolute: boolean;
}

export async function proofListProfiles(): Promise<ProofProfileInfo[]> {
  return invoke<ProofProfileInfo[]>('proof_list_profiles');
}

export async function proofImportProfile(path: string): Promise<ProofProfileInfo> {
  return invoke<ProofProfileInfo>('proof_import_profile', { path });
}

export async function proofRemoveProfile(id: string): Promise<void> {
  return invoke<void>('proof_remove_profile', { id });
}

export async function proofGetConfig(docId: number): Promise<SoftProofConfig> {
  // Tauri 2 maps Rust `doc_id` → JS `docId`.
  return invoke<SoftProofConfig>('proof_get_config', { docId });
}

export async function proofSetConfig(
  docId: number,
  config: SoftProofConfig,
): Promise<SoftProofConfig> {
  return invoke<SoftProofConfig>('proof_set_config', { docId, config });
}

export function softProofChipLabel(
  config: SoftProofConfig,
  profiles: ProofProfileInfo[],
): string {
  const name =
    profiles.find((p) => p.id === config.profile_id)?.name ?? config.profile_id;
  const short =
    config.intent === 'relative'
      ? 'Rel'
      : config.intent === 'perceptual'
        ? 'Perc'
        : 'Abs';
  const bpc = config.intent === 'relative' && config.bpc ? '+BPC' : '';
  return `${name} · ${short}${bpc}`;
}
