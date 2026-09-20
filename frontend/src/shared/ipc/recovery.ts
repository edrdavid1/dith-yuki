import { invoke } from '@tauri-apps/api/core';

export type JournalContentKind = 'full_dyproj' | 'skipped_too_large';

export interface JournalMeta {
  recovery_id: string;
  runtime_doc_id: number;
  display_name: string;
  project_path: string | null;
  source_path: string | null;
  original_mtime_ms: number | null;
  written_at_ms: number;
  app_version: string;
  content_kind: JournalContentKind;
  discarded: boolean;
}

export interface RosterEntry {
  recovery_id: string;
  runtime_doc_id: number;
  display_name: string;
  project_path: string | null;
  source_path: string | null;
  dirty: boolean;
}

export interface SessionRoster {
  open_docs: RosterEntry[];
  active_recovery_id: string | null;
}

export interface RecoveryScan {
  previous_unclean: boolean;
  journals: JournalMeta[];
  roster: SessionRoster | null;
}

export interface RecoveredProject {
  doc_id: number;
  width: number;
  height: number;
  path: string;
}

export async function scanRecoveryJournals(): Promise<RecoveryScan> {
  return invoke<RecoveryScan>('scan_recovery_journals');
}

export async function recoverJournal(recoveryId: string): Promise<RecoveredProject> {
  return invoke<RecoveredProject>('recover_journal', { recoveryId });
}

export async function discardRecoveryJournals(recoveryIds?: string[]): Promise<void> {
  return invoke('discard_recovery_journals', {
    recoveryIds: recoveryIds ?? null,
  });
}

export interface SoftDiscardInfo {
  recovery_id: string;
  display_name: string;
}

/** Flush journal now and mark discarded before closing a dirty tab. */
export async function prepareSoftDiscard(docId: number): Promise<SoftDiscardInfo> {
  return invoke<SoftDiscardInfo>('prepare_soft_discard', { docId });
}
